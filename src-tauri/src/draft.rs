//! 자주 쓰는 기안(연차휴가신청서, 동아리 활동 내역서)을 그룹웨어 창에 미리 채워 여는 기능입니다.
//!
//! 앱은 보이는 그룹웨어 창(`draft`)을 양식 주소로 새로 열고, 페이지가 다 불러와지면 채우기 스크립트를
//! `eval` 로 넣습니다. 스크립트는 진행 상황을 페이지 전역 `window.__amaranthDraftStatus` 에만 적고,
//! Rust 감시 작업이 이 값을 읽어 앱 화면에 알립니다. 상신·결재상신·신청완료·임시저장·저장·삭제는
//! 누르지 않으며, 사용자가 창에서 확인한 뒤 직접 누릅니다.
//!
//! 세션은 웹뷰 쿠키에 맡깁니다. 세션이 없어 로그인 화면으로 가면, 저장된 자격 증명이 있으면 기존
//! 자동 로그인 스크립트를 넣고, 없으면 사용자가 창에서 로그인하기를 기다린 뒤 양식을 다시 엽니다.

use crate::attendance::groupware::ORIGIN;
use crate::groupware::{
    auto_login_script, eval_string, failed, load_credentials, url_is_login, GroupwareError,
    AUTO_LOGIN_TIMEOUT, PAGE_HELPERS_JS,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{
    webview::{NewWindowFeatures, NewWindowResponse, PageLoadEvent},
    AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder, Wry,
};

const WINDOW_LABEL: &str = "draft";
/// 기안 창에서 `window.open` 으로 여는 그룹웨어 팝업(결재 작성 창 등)의 라벨 앞부분입니다.
const POPUP_LABEL_PREFIX: &str = "draft-popup-";
const LEAVE_PATH: &str =
    "/#/HP/HPD0110/HPD0110?MicroModuleCode=eap&docWidth=1035&formDTp=HP_HPD0110_00011&formId=36";
const CLUB_PATH: &str =
    "/#/HP/APB1020/APB1020?MicroModuleCode=eap&docWidth=1035&formDTp=APB1020_00001&formId=201";
const DRAFT_COMMON_JS: &str = include_str!("scripts/draft-common.js");
const LEAVE_JS: &str = include_str!("scripts/leave.js");
const CLUB_JS: &str = include_str!("scripts/club.js");
const ATTACH_JS: &str = include_str!("scripts/attach.js");
/// 영수증 파일 한 개의 최대 크기와 전체 최대 크기(바이트).
const RECEIPT_MAX_FILE: u64 = 20 * 1024 * 1024;
const RECEIPT_MAX_TOTAL: u64 = 50 * 1024 * 1024;
/// 첨부 스크립트 결과를 기다리는 최대 시간입니다.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(90);
/// 사용자가 직접 로그인하기를 기다리는 최대 시간입니다.
const LOGIN_WAIT: Duration = Duration::from_secs(300);
/// 양식이 열린 뒤 채우기가 끝날 때까지 기다리는 최대 시간입니다(스크립트 안의 대기도 각각 제한이 있음).
const FILL_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_millis(500);
/// 동아리 지출내역 용도 코드(이 양식에 연결된 유일한 용도).
const CLUB_CASH_TYPE_CODE: &str = "2204";
const CLUB_CASH_TYPE_NAME: &str = "복리후생비_동아리";

/// 기안 작성 화면에서 기억해 두는 값입니다. 비밀번호 같은 비밀 값은 넣지 않습니다.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct DraftSettings {
    /// 마지막으로 쓴 연차 사유
    pub leave_reason: String,
    pub club_name: String,
    /// 카드사용내역 팝업의 카드 목록에서 이 글자가 들어간 카드를 고릅니다.
    pub card_name: String,
    pub attendees: Vec<String>,
    /// 1인당 금액(원)
    pub per_person: u32,
}

impl Default for DraftSettings {
    fn default() -> Self {
        Self {
            leave_reason: String::new(),
            club_name: "커피".to_owned(),
            card_name: String::new(),
            attendees: Vec::new(),
            per_person: 30000,
        }
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, GroupwareError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join("draft.json"))
        .map_err(|error| failed(format!("설정 폴더를 찾을 수 없습니다: {error}")))
}

pub fn load_settings(app: &AppHandle) -> Result<DraftSettings, GroupwareError> {
    match fs::read(settings_path(app)?) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DraftSettings::default()),
        Err(error) => Err(failed(format!("기안 설정을 읽을 수 없습니다: {error}"))),
    }
}

pub fn save_settings(app: &AppHandle, settings: &DraftSettings) -> Result<(), GroupwareError> {
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| failed(format!("설정 폴더를 만들 수 없습니다: {error}")))?;
    }
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|error| failed(format!("기안 설정을 변환할 수 없습니다: {error}")))?;
    fs::write(&path, json).map_err(|error| failed(format!("기안 설정을 저장할 수 없습니다: {error}")))
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LeaveKind {
    Annual,
    AmHalf,
    PmHalf,
}

impl LeaveKind {
    /// 그룹웨어 휴가구분 버튼 글자
    fn label(self) -> &'static str {
        match self {
            Self::Annual => "연차",
            Self::AmHalf => "오전반차",
            Self::PmHalf => "오후반차",
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LeaveRequest {
    pub kind: LeaveKind,
    /// YYYY-MM-DD
    pub from: String,
    /// YYYY-MM-DD. 반차면 무시하고 시작일과 같게 둡니다.
    pub to: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ClubRequest {
    pub year: i32,
    pub month: u32,
    pub club_name: String,
    pub card_name: String,
    pub attendees: Vec<String>,
    pub per_person: u32,
    /// 결재 작성 창에 첨부할 영수증 파일 경로. 저장하지 않습니다.
    #[serde(default)]
    pub receipts: Vec<String>,
}

/// 결재 작성 창에 넣을 영수증 한 개(파일 이름·MIME·base64 내용).
#[derive(Serialize, Clone, Debug)]
struct ReceiptFile {
    name: String,
    #[serde(rename = "type")]
    mime: &'static str,
    data: String,
}

/// 기안을 연 뒤 결재 작성 창이 뜨면 첨부할 영수증. 기안 창을 새로 열면 바뀝니다.
struct PendingReceipts {
    id: u64,
    files: Vec<ReceiptFile>,
}

static PENDING_RECEIPTS: Mutex<Option<PendingReceipts>> = Mutex::new(None);

/// 영수증으로 받을 수 있는 확장자의 MIME 형식을 돌려줍니다. 받을 수 없으면 None입니다.
pub fn receipt_mime(file_name: &str) -> Option<&'static str> {
    let extension = file_name.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "heic" => Some("image/heic"),
        "pdf" => Some("application/pdf"),
        _ => None,
    }
}

/// 파일별(20MB)·전체(50MB) 크기 제한을 확인합니다. `files` 는 (파일 이름, 바이트 수) 목록입니다.
pub fn check_receipt_sizes(files: &[(String, u64)]) -> Result<(), GroupwareError> {
    for (name, size) in files {
        if *size > RECEIPT_MAX_FILE {
            return Err(failed(format!("영수증 파일이 20MB를 넘습니다: {name}")));
        }
    }
    let total: u64 = files.iter().map(|(_, size)| size).sum();
    if total > RECEIPT_MAX_TOTAL {
        return Err(failed("영수증 파일 크기의 합이 50MB를 넘습니다. 파일 수를 줄이세요"));
    }
    Ok(())
}

/// 고른 영수증 파일을 검사하고 읽어 base64 로 바꿉니다.
fn read_receipts(paths: &[String]) -> Result<Vec<ReceiptFile>, GroupwareError> {
    let mut entries = Vec::new();
    for path in paths {
        let path = PathBuf::from(path);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| failed("영수증 파일 이름을 읽을 수 없습니다"))?;
        let mime = receipt_mime(&name)
            .ok_or_else(|| failed(format!("영수증으로 첨부할 수 없는 파일 형식입니다: {name} (jpg·png·gif·heic·pdf)")))?;
        let size = fs::metadata(&path)
            .map_err(|error| failed(format!("영수증 파일을 읽을 수 없습니다: {name} ({error})")))?
            .len();
        entries.push((path, name, mime, size));
    }
    let sizes: Vec<(String, u64)> = entries.iter().map(|(_, name, _, size)| (name.clone(), *size)).collect();
    check_receipt_sizes(&sizes)?;
    entries
        .into_iter()
        .map(|(path, name, mime, _)| {
            let bytes = fs::read(&path)
                .map_err(|error| failed(format!("영수증 파일을 읽을 수 없습니다: {name} ({error})")))?;
            Ok(ReceiptFile { name, mime, data: STANDARD.encode(bytes) })
        })
        .collect()
}

fn attach_script(files: &[ReceiptFile]) -> String {
    fill_script(&serde_json::json!({ "files": files }), ATTACH_JS)
}

/// 결재 작성 창(UBAP001) 주소인지 확인합니다.
fn is_approval_editor(url: &Url) -> bool {
    let fragment = url.fragment().unwrap_or_default();
    is_groupware(url) && fragment.contains("/popup") && fragment.contains("callComp=UBAP001")
}

/// 앱 화면에 보여 줄 기안 창 진행 상황입니다.
/// `state`: idle(시작 전) · opening · login · filling · done · empty(반영할 카드 내역 없음) · failed · closed
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DraftStatus {
    pub id: u64,
    pub state: String,
    pub step: String,
    pub message: String,
}

static STATUS: Mutex<DraftStatus> = Mutex::new(DraftStatus {
    id: 0,
    state: String::new(),
    step: String::new(),
    message: String::new(),
});
/// 기안 창을 새로 열 때마다 늘어납니다. 이전 창의 감시 작업이 새 상태를 덮어쓰지 않게 합니다.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static POPUP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn set_status(id: u64, state: &str, step: &str, message: &str) {
    if GENERATION.load(Ordering::SeqCst) != id {
        return;
    }
    if let Ok(mut status) = STATUS.lock() {
        *status = DraftStatus {
            id,
            state: state.to_owned(),
            step: step.to_owned(),
            message: message.to_owned(),
        };
    }
}

pub fn status() -> DraftStatus {
    let status = STATUS.lock().map(|status| status.clone()).unwrap_or(DraftStatus {
        id: 0,
        state: String::new(),
        step: String::new(),
        message: String::new(),
    });
    if status.state.is_empty() {
        DraftStatus { state: "idle".to_owned(), ..status }
    } else {
        status
    }
}

fn parse_date(value: &str, label: &str) -> Result<NaiveDate, GroupwareError> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .map_err(|_| failed(format!("{label} 형식이 올바르지 않습니다 (YYYY-MM-DD)")))
}

fn compact(date: NaiveDate) -> String {
    date.format("%Y%m%d").to_string()
}

/// 해당 월의 1일과 말일을 돌려줍니다.
pub fn month_bounds(year: i32, month: u32) -> Option<(NaiveDate, NaiveDate)> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    Some((first, next.pred_opt()?))
}

/// 페이지에 넣을 스크립트를 만듭니다. 값은 `serde_json` 으로 직렬화해 문자열 이스케이프를 맡깁니다.
fn fill_script(params: &serde_json::Value, body: &str) -> String {
    let params = serde_json::to_string(params).unwrap_or_else(|_| "{}".to_owned());
    format!("(function() {{\n  var params = {params};\n{PAGE_HELPERS_JS}\n{DRAFT_COMMON_JS}\n{body}\n}})();")
}

pub fn leave_script(request: &LeaveRequest) -> Result<String, GroupwareError> {
    let from = parse_date(&request.from, "시작일")?;
    let to = if request.kind == LeaveKind::Annual {
        parse_date(&request.to, "종료일")?
    } else {
        from
    };
    if to < from {
        return Err(failed("종료일이 시작일보다 빠릅니다"));
    }
    let params = serde_json::json!({
        "kind": request.kind.label(),
        "from": compact(from),
        "to": compact(to),
        "reason": request.reason.trim(),
    });
    Ok(fill_script(&params, LEAVE_JS))
}

pub fn club_title(club_name: &str, month: u32) -> String {
    format!("[동아리] [{club_name}] {month}월 활동 내역서")
}

pub fn club_script(request: &ClubRequest) -> Result<String, GroupwareError> {
    let (first, last) =
        month_bounds(request.year, request.month).ok_or_else(|| failed("대상 월이 올바르지 않습니다"))?;
    let club_name = request.club_name.trim();
    let card_name = request.card_name.trim();
    if club_name.is_empty() {
        return Err(failed("동아리명을 입력하세요"));
    }
    if card_name.is_empty() {
        return Err(failed("카드 이름을 입력하세요"));
    }
    let params = serde_json::json!({
        "title": club_title(club_name, request.month),
        "from": compact(first),
        "to": compact(last),
        "cardName": card_name,
        "memo": format!("{club_name}_동아리활동"),
        "cashType": {
            "typeCd": CLUB_CASH_TYPE_CODE,
            "typeNm": CLUB_CASH_TYPE_NAME,
            "deductFg": "", "taxFg": "", "nonsubTy": "",
            "deptCheckYn": "N", "pjtCheckYn": "N", "budgetCheckYn": "N",
            "trcdCheckYn": "N", "accountCheckYn": "N", "carYn": "N",
        },
        "confirmCards": true,
    });
    Ok(fill_script(&params, CLUB_JS))
}

/// 연차휴가신청서를 채워 엽니다. 사유는 다음에 쓰도록 기억해 둡니다.
pub async fn open_leave(app: &AppHandle, request: LeaveRequest) -> Result<(), GroupwareError> {
    let script = leave_script(&request)?;
    let mut settings = load_settings(app)?;
    settings.leave_reason = request.reason.trim().to_owned();
    save_settings(app, &settings)?;
    open_draft_window(app, LEAVE_PATH, script, Vec::new()).await
}

/// 동아리 활동 내역서를 채워 엽니다. 동아리명·카드 이름·참석자·1인당 금액은 기억해 둡니다.
pub async fn open_club(app: &AppHandle, request: ClubRequest) -> Result<(), GroupwareError> {
    let script = club_script(&request)?;
    let receipts = read_receipts(&request.receipts)?;
    let mut settings = load_settings(app)?;
    settings.club_name = request.club_name.trim().to_owned();
    settings.card_name = request.card_name.trim().to_owned();
    settings.attendees = request
        .attendees
        .iter()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect();
    settings.per_person = request.per_person;
    save_settings(app, &settings)?;
    open_draft_window(app, CLUB_PATH, script, receipts).await
}

fn groupware_host() -> Option<String> {
    ORIGIN.parse::<Url>().ok()?.host_str().map(str::to_owned)
}

fn is_groupware(url: &Url) -> bool {
    url.host_str().is_some() && url.host_str().map(str::to_owned) == groupware_host()
}

/// 기안 창이나 그 팝업에서 `window.open` 을 부르면, 그룹웨어 주소일 때만 같은 쿠키 저장소를 쓰는
/// 새 Tauri 창으로 엽니다(`window_features` 가 macOS 에서는 여는 쪽 웹뷰 설정을, Windows 에서는 같은
/// 환경을 이어받게 합니다). 그 밖의 주소는 열지 않습니다.
fn open_popup(app: &AppHandle, url: Url, features: NewWindowFeatures) -> NewWindowResponse<Wry> {
    if !(is_groupware(&url) || url.as_str() == "about:blank") {
        return NewWindowResponse::Deny;
    }
    let Ok(blank) = "about:blank".parse::<Url>() else {
        return NewWindowResponse::Deny;
    };
    let label = format!("{POPUP_LABEL_PREFIX}{}", POPUP_COUNTER.fetch_add(1, Ordering::SeqCst));
    let handle = app.clone();
    let attach_app = app.clone();
    let attached = Arc::new(AtomicBool::new(false));
    let built = WebviewWindowBuilder::new(app, label, WebviewUrl::External(blank))
        .window_features(features)
        .title("그룹웨어")
        .on_document_title_changed(|window, title| {
            let _ = window.set_title(&title);
        })
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished || !is_approval_editor(payload.url()) {
                return;
            }
            // 팝업 창마다 한 번만 첨부합니다.
            if attached.swap(true, Ordering::SeqCst) {
                return;
            }
            let pending = PENDING_RECEIPTS.lock().ok().and_then(|slot| {
                slot.as_ref()
                    .filter(|pending| !pending.files.is_empty())
                    .map(|pending| (pending.id, pending.files.clone()))
            });
            let Some((id, files)) = pending else {
                return;
            };
            let count = files.len();
            let _ = window.eval(attach_script(&files));
            let app = attach_app.clone();
            let label = window.label().to_owned();
            tauri::async_runtime::spawn(async move { watch_attach(app, id, label, count).await });
        })
        .on_new_window(move |url, features| open_popup(&handle, url, features))
        .build();
    match built {
        Ok(window) => NewWindowResponse::Create { window },
        Err(_) => NewWindowResponse::Deny,
    }
}

/// 이전 기안 창과 팝업을 닫습니다. 직접 주소는 새로 불러올 때만 양식이 열리므로 늘 새 창을 씁니다.
async fn close_previous(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label == WINDOW_LABEL || label.starts_with(POPUP_LABEL_PREFIX) {
            let _ = window.destroy();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.get_webview_window(WINDOW_LABEL).is_some() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn open_draft_window(
    app: &AppHandle,
    path: &str,
    script: String,
    receipts: Vec<ReceiptFile>,
) -> Result<(), GroupwareError> {
    let id = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let receipt_count = receipts.len();
    if let Ok(mut slot) = PENDING_RECEIPTS.lock() {
        *slot = Some(PendingReceipts { id, files: receipts });
    }
    set_status(id, "opening", "창 열기", "");
    close_previous(app).await;
    let form_url: Url = format!("{ORIGIN}{path}")
        .parse()
        .map_err(|error| failed(format!("그룹웨어 주소가 올바르지 않습니다: {error}")))?;
    let popup_app = app.clone();
    let built = WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(form_url.clone()))
        .title("그룹웨어 기안 작성")
        .inner_size(1280.0, 900.0)
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished
                || !is_groupware(payload.url())
                || url_is_login(payload.url())
            {
                return;
            }
            // 페이지마다 한 번만 실행되도록 스크립트 안에서 막습니다. 로그인 화면으로 바뀌면 스스로 멈춥니다.
            let _ = window.eval(script.clone());
        })
        .on_new_window(move |url, features| open_popup(&popup_app, url, features))
        .build();
    if let Err(error) = built {
        set_status(id, "failed", "창 열기", &format!("기안 창을 열 수 없습니다: {error}"));
        return Err(failed(format!("기안 창을 열 수 없습니다: {error}")));
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { supervise(app, id, form_url, receipt_count).await });
    Ok(())
}

#[derive(Deserialize, Default)]
struct PageStatus {
    #[serde(default)]
    state: String,
    #[serde(default)]
    step: String,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct Probe {
    href: String,
    status: Option<PageStatus>,
}

const PROBE_JS: &str =
    "(function(){try{return JSON.stringify({href:String(location.href),status:window.__amaranthDraftStatus||null})}catch(e){return ''}})()";

/// 기안 창을 지켜보며 로그인 처리와 채우기 결과를 상태에 옮깁니다. 창이 닫히거나 끝나면 멈춥니다.
async fn supervise(app: AppHandle, id: u64, form_url: Url, receipt_count: usize) {
    let credentials = load_credentials(&app);
    let mut login_since: Option<Instant> = None;
    let mut auto_attempted = false;
    let mut fill_since = Instant::now();
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        if GENERATION.load(Ordering::SeqCst) != id {
            return;
        }
        let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
            set_status(id, "closed", "", "기안 창이 닫혔습니다");
            return;
        };
        let Some(probe) = eval_string(&window, PROBE_JS)
            .await
            .and_then(|text| serde_json::from_str::<Probe>(&text).ok())
        else {
            continue;
        };
        let Ok(href) = probe.href.parse::<Url>() else {
            continue;
        };

        if is_groupware(&href) && url_is_login(&href) {
            let since = *login_since.get_or_insert_with(Instant::now);
            if since.elapsed() > LOGIN_WAIT {
                set_status(id, "failed", "로그인", "로그인을 기다리다 시간이 지났습니다. 다시 시도하세요");
                return;
            }
            match &credentials {
                Some((login_id, password)) if !auto_attempted => {
                    auto_attempted = true;
                    let _ = window.eval(auto_login_script(login_id, password));
                    set_status(id, "login", "로그인", "저장된 계정으로 로그인하는 중…");
                }
                Some(_) if since.elapsed() > AUTO_LOGIN_TIMEOUT => {
                    set_status(id, "login", "로그인", "자동 로그인에 실패했습니다. 그룹웨어 창에서 직접 로그인하세요");
                }
                Some(_) => {}
                None => set_status(id, "login", "로그인", "그룹웨어 창에서 로그인하면 양식을 엽니다"),
            }
            let _ = window.show();
            continue;
        }

        if login_since.take().is_some() {
            // 로그인을 마쳤습니다. 양식은 새로 불러올 때만 열리므로 about:blank 를 거쳐 다시 엽니다.
            set_status(id, "filling", "양식 열기", "");
            tokio::time::sleep(Duration::from_secs(2)).await;
            if let Ok(blank) = "about:blank".parse::<Url>() {
                let _ = window.navigate(blank);
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
            let _ = window.navigate(form_url.clone());
            auto_attempted = false;
            fill_since = Instant::now();
            continue;
        }

        if let Some(page) = probe.status {
            match page.state.as_str() {
                "done" if receipt_count > 0 => {
                    set_status(
                        id,
                        "awaitingReceipts",
                        "영수증 첨부",
                        &format!(
                            "그룹웨어 창에서 내용을 확인하고 결재상신을 누르면, 결재 작성 창에 영수증 {receipt_count}개를 첨부합니다"
                        ),
                    );
                    let _ = window.set_focus();
                    wait_for_editor(&app, id).await;
                    return;
                }
                "done" | "empty" | "failed" => {
                    set_status(id, &page.state, &page.step, &page.message);
                    let _ = window.set_focus();
                    return;
                }
                "filling" => set_status(id, "filling", &page.step, &page.message),
                _ => {}
            }
        }
        if fill_since.elapsed() > FILL_TIMEOUT {
            set_status(id, "failed", "양식 채우기", "양식 채우기가 제한 시간 안에 끝나지 않았습니다");
            return;
        }
    }
}

/// 결재 작성 창이 뜰 때까지 기다립니다. 첨부가 시작되면(상태가 바뀌면) 멈추고, 기안 창과 팝업이
/// 모두 닫히면 「닫힘」으로 알립니다.
async fn wait_for_editor(app: &AppHandle, id: u64) {
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        if GENERATION.load(Ordering::SeqCst) != id || status().state != "awaitingReceipts" {
            return;
        }
        let any_open = app
            .webview_windows()
            .keys()
            .any(|label| label == WINDOW_LABEL || label.starts_with(POPUP_LABEL_PREFIX));
        if !any_open {
            set_status(id, "closed", "", "기안 창이 닫혔습니다");
            return;
        }
    }
}

/// 결재 작성 창에서 첨부 스크립트 결과를 읽어 상태에 옮깁니다.
async fn watch_attach(app: AppHandle, id: u64, label: String, count: usize) {
    set_status(id, "filling", "영수증 첨부", "");
    let started = Instant::now();
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        if GENERATION.load(Ordering::SeqCst) != id {
            return;
        }
        let Some(window) = app.get_webview_window(&label) else {
            set_status(id, "failed", "영수증 첨부", "결재 작성 창이 닫혔습니다");
            return;
        };
        let page = eval_string(&window, PROBE_JS)
            .await
            .and_then(|text| serde_json::from_str::<Probe>(&text).ok())
            .and_then(|probe| probe.status);
        if let Some(page) = page {
            match page.state.as_str() {
                "done" => {
                    let message = format!("영수증 {count}개를 첨부했습니다. 확인 후 상신하세요");
                    set_status(id, "done", "영수증 첨부", &message);
                    return;
                }
                "failed" => {
                    set_status(id, "failed", &page.step, &page.message);
                    return;
                }
                _ => {}
            }
        }
        if started.elapsed() > ATTACH_TIMEOUT {
            set_status(id, "failed", "영수증 첨부", "영수증 첨부가 제한 시간 안에 끝나지 않았습니다");
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 스크립트가 제출·저장 계열 버튼 글자를 담고 있지 않은지 확인합니다(누를 수 없게).
    fn assert_no_submit_words(script: &str) {
        for word in ["상신", "신청완료", "임시저장", "저장", "삭제", "보관"] {
            assert!(!script.contains(word), "스크립트에 「{word}」가 들어 있습니다");
        }
    }

    #[test]
    fn leave_script_uses_compact_dates_and_label() {
        let script = leave_script(&LeaveRequest {
            kind: LeaveKind::Annual,
            from: "2026-12-01".to_owned(),
            to: "2026-12-02".to_owned(),
            reason: "개인 \"사유\" </script>".to_owned(),
        })
        .unwrap();
        assert!(script.contains(r#""from":"20261201""#));
        assert!(script.contains(r#""to":"20261202""#));
        assert!(script.contains(r#""kind":"연차""#));
        assert!(script.contains(r#"개인 \"사유\""#));
        assert_no_submit_words(&script);
    }

    #[test]
    fn half_day_uses_single_date() {
        let script = leave_script(&LeaveRequest {
            kind: LeaveKind::PmHalf,
            from: "2026-12-03".to_owned(),
            to: "2026-12-09".to_owned(),
            reason: String::new(),
        })
        .unwrap();
        assert!(script.contains(r#""to":"20261203""#));
        assert!(script.contains(r#""kind":"오후반차""#));
    }

    #[test]
    fn leave_rejects_reversed_range() {
        assert!(leave_script(&LeaveRequest {
            kind: LeaveKind::Annual,
            from: "2026-12-03".to_owned(),
            to: "2026-12-01".to_owned(),
            reason: String::new(),
        })
        .is_err());
    }

    #[test]
    fn club_script_covers_whole_month() {
        let script = club_script(&ClubRequest {
            year: 2026,
            month: 2,
            club_name: "커피".to_owned(),
            card_name: "동아리_카드".to_owned(),
            attendees: vec!["A".to_owned()],
            per_person: 30000,
            receipts: Vec::new(),
        })
        .unwrap();
        assert!(script.contains(r#""from":"20260201""#));
        assert!(script.contains(r#""to":"20260228""#));
        assert!(script.contains("[동아리] [커피] 2월 활동 내역서"));
        assert!(script.contains(r#""memo":"커피_동아리활동""#));
        assert_no_submit_words(&script);
    }

    #[test]
    fn month_bounds_cover_whole_month() {
        assert_eq!(
            month_bounds(2024, 12),
            Some((NaiveDate::from_ymd_opt(2024, 12, 1).unwrap(), NaiveDate::from_ymd_opt(2024, 12, 31).unwrap()))
        );
        assert_eq!(month_bounds(2024, 13), None);
    }

    #[test]
    fn receipt_mime_accepts_images_and_pdf_only() {
        assert_eq!(receipt_mime("영수증.JPG"), Some("image/jpeg"));
        assert_eq!(receipt_mime("a.jpeg"), Some("image/jpeg"));
        assert_eq!(receipt_mime("a.png"), Some("image/png"));
        assert_eq!(receipt_mime("a.gif"), Some("image/gif"));
        assert_eq!(receipt_mime("IMG_0001.HEIC"), Some("image/heic"));
        assert_eq!(receipt_mime("a.pdf"), Some("application/pdf"));
        assert_eq!(receipt_mime("a.exe"), None);
        assert_eq!(receipt_mime("noext"), None);
    }

    #[test]
    fn receipt_sizes_respect_limits() {
        const MB: u64 = 1024 * 1024;
        assert!(check_receipt_sizes(&[("a.jpg".to_owned(), 20 * MB)]).is_ok());
        let too_big = check_receipt_sizes(&[("big.jpg".to_owned(), 20 * MB + 1)]).unwrap_err();
        assert!(too_big.message().contains("big.jpg"));
        let files: Vec<(String, u64)> = (0..3).map(|index| (format!("{index}.jpg"), 18 * MB)).collect();
        assert!(check_receipt_sizes(&files).unwrap_err().message().contains("50MB"));
        assert!(check_receipt_sizes(&[]).is_ok());
    }

    #[test]
    fn attach_script_escapes_names_and_has_no_submit_words() {
        let script = attach_script(&[ReceiptFile {
            name: "영수증 \"1\".jpg".to_owned(),
            mime: "image/jpeg",
            data: STANDARD.encode(b"abc"),
        }]);
        assert!(script.contains(r#""name":"영수증 \"1\".jpg""#));
        assert!(script.contains(r#""type":"image/jpeg""#));
        assert!(script.contains(r#""data":"YWJj""#));
        assert_no_submit_words(&script);
    }

    #[test]
    fn approval_editor_url_is_detected() {
        let editor: Url = "https://gwa.soomgo.com/#/popup?MicroModuleCode=eap&formId=201&callComp=UBAP001&popupUUID=x"
            .parse()
            .unwrap();
        let form: Url = format!("{ORIGIN}{CLUB_PATH}").parse().unwrap();
        assert!(is_approval_editor(&editor));
        assert!(!is_approval_editor(&form));
    }

    #[test]
    fn auto_login_script_includes_shared_helpers() {
        let script = auto_login_script("id", "p\"w");
        assert!(script.contains("function setValue"));
        assert!(script.contains(r#"var password = "p\"w";"#));
    }
}

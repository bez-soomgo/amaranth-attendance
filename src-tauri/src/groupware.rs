use crate::attendance::{
    groupware::{
        decode_cookie, detail_request, month_range, read_detail_rows, read_session_user,
        read_today_arrival, sign, today_request, DETAIL_PATH, ORIGIN, TODAY_PATH,
    },
    model::RawRow,
};
use chrono::{Local, Utc};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{webview::PageLoadEvent, AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};

const WINDOW_LABEL: &str = "groupware";
/// OS 키체인(macOS 키체인·Windows 자격 증명 관리자)에 비밀번호를 저장할 때 쓰는 서비스 이름. 계정명은 로그인 아이디입니다.
const CREDENTIAL_SERVICE: &str = "io.github.bezsoomgo.amaranthattendance.groupware";
/// 자동 로그인 뒤 이 시간이 지나도 로그인 화면에 머물러 있으면(비밀번호 틀림 등) 창을 보여 줍니다.
const AUTO_LOGIN_TIMEOUT: Duration = Duration::from_secs(15);

/// 로그인 창에서 얻은 그룹웨어 세션. 사번과 회사 코드도 사용자 입력이 아니라 이 세션에서 읽습니다.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Session {
    token: String,
    sign_key: String,
    emp_code: String,
    company_code: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Settings {
    session: Option<Session>,
    /// 자동 로그인에 쓸 아이디. 비밀번호는 여기 저장하지 않고 OS 키체인에만 둡니다.
    login_id: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum GroupwareError {
    #[serde(rename_all = "camelCase")]
    LoginRequired { window_open: bool },
    Failed { message: String },
}

impl GroupwareError {
    pub fn message(&self) -> String {
        match self {
            Self::LoginRequired { .. } => "그룹웨어 로그인이 필요합니다".to_owned(),
            Self::Failed { message } => message.clone(),
        }
    }
}

fn failed(message: impl Into<String>) -> GroupwareError {
    GroupwareError::Failed { message: message.into() }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, GroupwareError> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join("groupware.json"))
        .map_err(|error| failed(format!("설정 폴더를 찾을 수 없습니다: {error}")))
}

fn load_settings(app: &AppHandle) -> Result<Settings, GroupwareError> {
    match fs::read(settings_path(app)?) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(failed(format!("그룹웨어 설정을 읽을 수 없습니다: {error}"))),
    }
}

fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), GroupwareError> {
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| failed(format!("설정 폴더를 만들 수 없습니다: {error}")))?;
    }
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|error| failed(format!("그룹웨어 설정을 변환할 수 없습니다: {error}")))?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(&path)
        .map_err(|error| failed(format!("그룹웨어 설정을 저장할 수 없습니다: {error}")))?;
    std::io::Write::write_all(&mut file, &json)
        .map_err(|error| failed(format!("그룹웨어 설정을 저장할 수 없습니다: {error}")))
}

fn credential_entry(login_id: &str) -> Result<Entry, GroupwareError> {
    Entry::new(CREDENTIAL_SERVICE, login_id)
        .map_err(|error| failed(format!("키체인에 접근할 수 없습니다: {error}")))
}

/// 아이디는 `groupware.json`에, 비밀번호는 OS 키체인에 저장합니다. 비밀번호는 설정 파일에 쓰지 않습니다.
pub fn save_credentials(app: &AppHandle, login_id: &str, password: &str) -> Result<(), GroupwareError> {
    credential_entry(login_id)?
        .set_password(password)
        .map_err(|error| failed(format!("자격 증명을 저장할 수 없습니다: {error}")))?;
    let mut settings = load_settings(app)?;
    // 아이디를 바꿔 저장하면 이전 아이디의 키체인 항목을 지웁니다.
    if let Some(previous) = settings.login_id.as_deref().filter(|previous| *previous != login_id) {
        if let Ok(entry) = credential_entry(previous) {
            let _ = entry.delete_credential();
        }
    }
    settings.login_id = Some(login_id.to_owned());
    save_settings(app, &settings)
}

/// 저장된 자격 증명을 삭제합니다. 키체인 항목이 이미 없어도 실패로 보지 않습니다.
pub fn clear_credentials(app: &AppHandle) -> Result<(), GroupwareError> {
    let mut settings = load_settings(app)?;
    if let Some(login_id) = settings.login_id.take() {
        match credential_entry(&login_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(error) => return Err(failed(format!("자격 증명을 삭제할 수 없습니다: {error}"))),
        }
    }
    save_settings(app, &settings)
}

/// 저장된 아이디를 돌려줍니다. 자동 로그인 설정 화면에 보여 줄 때 씁니다.
pub fn saved_login_id(app: &AppHandle) -> Result<Option<String>, GroupwareError> {
    Ok(load_settings(app)?.login_id)
}

/// 저장된 아이디·비밀번호를 함께 읽습니다. 아이디는 있지만 키체인에 비밀번호가 없으면(삭제됨 등) None입니다.
fn load_credentials(app: &AppHandle) -> Option<(String, String)> {
    let login_id = load_settings(app).ok()?.login_id?;
    let password = credential_entry(&login_id).ok()?.get_password().ok()?;
    Some((login_id, password))
}

/// 로그인 창의 sessionStorage `userInfo`를 문자열로 읽습니다. 페이지에 앱 기능을 노출하지 않고 값만 돌려받습니다.
async fn window_user_info(window: &tauri::WebviewWindow) -> Option<String> {
    let (sender, receiver) = tokio::sync::oneshot::channel::<String>();
    let sender = Mutex::new(Some(sender));
    window
        .eval_with_callback(
            "(function(){try{return String(sessionStorage.getItem('userInfo')||'')}catch(e){return ''}})()",
            move |result| {
                if let Some(sender) = sender.lock().ok().and_then(|mut slot| slot.take()) {
                    let _ = sender.send(result);
                }
            },
        )
        .ok()?;
    let result = tokio::time::timeout(Duration::from_secs(3), receiver)
        .await
        .ok()?
        .ok()?;
    serde_json::from_str::<String>(&result)
        .ok()
        .filter(|text| !text.is_empty())
}

/// 로그인 창(웹뷰)이 로그인 화면을 벗어난 뒤 남아 있는 그룹웨어 세션과 본인 사번을 읽습니다.
async fn window_session(app: &AppHandle) -> Option<Session> {
    let window = app.get_webview_window(WINDOW_LABEL)?;
    let current = window.url().ok()?;
    if current.fragment().unwrap_or_default().contains("login") || current.path().contains("login") {
        return None;
    }
    let url: Url = ORIGIN.parse().ok()?;
    let cookies = window.cookies_for_url(url).ok()?;
    let value = |name: &str| {
        cookies
            .iter()
            .find(|cookie| cookie.name() == name)
            .map(|cookie| decode_cookie(cookie.value()))
            .filter(|value| !value.is_empty())
    };
    let token = value("oAuthToken")?;
    let sign_key = value("signKey")?;
    let user = read_session_user(&window_user_info(&window).await?)?;
    Some(Session {
        token,
        sign_key,
        emp_code: user.emp_code,
        company_code: user.company_code,
    })
}

enum CallError {
    Unauthorized,
    Failed(String),
}

pub struct GroupwareData {
    pub rows: Vec<RawRow>,
    /// 메인 화면 기준 오늘 출근 시각(분). 출근 전이거나 조회에 실패하면 None입니다.
    pub today_arrival: Option<i32>,
}

async fn post(session: &Session, path: &str, body: &serde_json::Value) -> Result<Vec<u8>, CallError> {
    let transaction_id = uuid::Uuid::new_v4().simple().to_string();
    let timestamp = Utc::now().timestamp().to_string();
    let signature = sign(&session.token, &transaction_id, &timestamp, path, &session.sign_key);
    let response = reqwest::Client::new()
        .post(format!("{ORIGIN}{path}"))
        .header("authorization", format!("Bearer {}", session.token))
        .header("timestamp", timestamp)
        .header("transaction-id", transaction_id)
        .header("wehago-sign", signature)
        .header("origin", ORIGIN)
        .header("referer", format!("{ORIGIN}/"))
        .json(body)
        .send()
        .await
        .map_err(|error| CallError::Failed(format!("그룹웨어에 연결할 수 없습니다: {error}")))?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(CallError::Unauthorized);
    }
    if !status.is_success() {
        return Err(CallError::Failed(format!("그룹웨어 응답 오류: HTTP {status}")));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| CallError::Failed(format!("그룹웨어 응답을 읽을 수 없습니다: {error}")))
}

async fn fetch_detail(session: &Session) -> Result<Vec<RawRow>, CallError> {
    let (from, to) = month_range(Local::now().date_naive());
    let body = post(session, DETAIL_PATH, &detail_request(&session.emp_code, from, to)).await?;
    read_detail_rows(&body).map_err(CallError::Failed)
}

async fn fetch_today_arrival(session: &Session) -> Option<i32> {
    let today = Local::now().date_naive();
    let request = today_request(&session.emp_code, &session.company_code, today);
    let body = post(session, TODAY_PATH, &request).await.ok()?;
    read_today_arrival(&body, today)
}

/// 이번 달 출결 행과 오늘 출근 시각을 가져옵니다.
pub async fn fetch(app: &AppHandle) -> Result<GroupwareData, GroupwareError> {
    let (rows, session) = fetch_with_session(app).await?;
    let today_arrival = fetch_today_arrival(&session).await;
    Ok(GroupwareData { rows, today_arrival })
}

pub async fn fetch_rows(app: &AppHandle) -> Result<Vec<RawRow>, GroupwareError> {
    fetch_with_session(app).await.map(|(rows, _)| rows)
}

/// 저장된 세션 → 로그인 창 세션 순으로 시도해 이번 달 출결 행을 가져옵니다.
async fn fetch_with_session(app: &AppHandle) -> Result<(Vec<RawRow>, Session), GroupwareError> {
    let mut settings = load_settings(app)?;
    let stored = settings.session.clone();
    if let Some(session) = &stored {
        match fetch_detail(session).await {
            Ok(rows) => return Ok((rows, session.clone())),
            Err(CallError::Failed(message)) => return Err(failed(message)),
            Err(CallError::Unauthorized) => {
                settings.session = None;
                save_settings(app, &settings)?;
            }
        }
    }

    let window_open = app.get_webview_window(WINDOW_LABEL).is_some();
    let Some(session) = window_session(app)
        .await
        .filter(|found| Some(found) != stored.as_ref())
    else {
        return Err(GroupwareError::LoginRequired { window_open });
    };
    match fetch_detail(&session).await {
        Ok(rows) => {
            settings.session = Some(session.clone());
            save_settings(app, &settings)?;
            if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
                let _ = window.close();
            }
            Ok((rows, session))
        }
        Err(CallError::Unauthorized) => Err(GroupwareError::LoginRequired { window_open }),
        Err(CallError::Failed(message)) => Err(failed(message)),
    }
}

fn url_is_login(url: &Url) -> bool {
    url.fragment().unwrap_or_default().contains("login") || url.path().contains("login")
}

/// 숨긴 로그인 창에서 아이디·비밀번호를 자동으로 채워 넣는 스크립트입니다. 입력칸이 React(OBT)
/// 컴포넌트라 `value` 대입만으로는 반영되지 않아 native setter + `input`/`change` 이벤트로 채웁니다.
/// 아이디 입력 → 「다음」 제출 → 비밀번호 칸이 보이면 입력 → 다시 제출 순서입니다.
fn auto_login_script(login_id: &str, password: &str) -> String {
    let login_id = serde_json::to_string(login_id).unwrap_or_else(|_| "\"\"".to_owned());
    let password = serde_json::to_string(password).unwrap_or_else(|_| "\"\"".to_owned());
    format!(
        r#"(function() {{
  var loginId = {login_id};
  var password = {password};
  function setValue(input, value) {{
    var setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
    setter.call(input, value);
    input.dispatchEvent(new Event('input', {{ bubbles: true }}));
    input.dispatchEvent(new Event('change', {{ bubbles: true }}));
  }}
  function waitFor(check, timeoutMs, intervalMs, onReady) {{
    var waited = 0;
    var timer = setInterval(function() {{
      var target = check();
      if (target) {{
        clearInterval(timer);
        onReady(target);
      }} else if ((waited += intervalMs) >= timeoutMs) {{
        clearInterval(timer);
      }}
    }}, intervalMs);
  }}
  waitFor(function() {{ return document.querySelector('#reqLoginId'); }}, 5000, 100, function(idInput) {{
    setValue(idInput, loginId);
    var idSubmit = document.querySelector('button.loginBtnFlex');
    if (idSubmit) idSubmit.click();
    waitFor(function() {{
      var pwInput = document.querySelector('#reqLoginPw');
      return pwInput && pwInput.offsetParent !== null ? pwInput : null;
    }}, 5000, 100, function(pwInput) {{
      setValue(pwInput, password);
      var pwSubmit = document.querySelector('button.loginBtnFlex');
      if (pwSubmit) pwSubmit.click();
    }});
  }});
}})();"#
    )
}

/// 로그인 창을 엽니다. 이미 열려 있으면 보여 주기만 합니다. 저장된 자격 증명이 있으면 창을 숨긴 채
/// 자동으로 로그인을 시도하고(`auto: true`), 15초가 지나도 로그인 화면에 머물러 있으면 창을 보여 줍니다.
/// 자격 증명이 없으면 지금까지처럼 창을 바로 보여 줍니다(`auto: false`).
pub fn open_login_window(app: &AppHandle) -> Result<bool, GroupwareError> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(false);
    }
    let url: Url = format!("{ORIGIN}/#/login")
        .parse()
        .map_err(|error| failed(format!("그룹웨어 주소가 올바르지 않습니다: {error}")))?;

    let Some((login_id, password)) = load_credentials(app) else {
        WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(url))
            .title("그룹웨어 로그인")
            .inner_size(1100.0, 800.0)
            .build()
            .map_err(|error| failed(format!("로그인 창을 열 수 없습니다: {error}")))?;
        return Ok(false);
    };

    let script = auto_login_script(&login_id, &password);
    let attempted = Arc::new(AtomicBool::new(false));
    WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(url))
        .title("그룹웨어 로그인")
        .inner_size(1100.0, 800.0)
        .visible(false)
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished || !url_is_login(payload.url()) {
                return;
            }
            // 창별로 한 번만 자동 입력합니다. 다시 로그인 화면으로 돌아와도 반복하지 않습니다.
            if attempted.swap(true, Ordering::SeqCst) {
                return;
            }
            let _ = window.eval(script.clone());
        })
        .build()
        .map_err(|error| failed(format!("로그인 창을 열 수 없습니다: {error}")))?;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(AUTO_LOGIN_TIMEOUT).await;
        let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
            return;
        };
        let still_on_login = window.url().map(|url| url_is_login(&url)).unwrap_or(false);
        if still_on_login {
            let _ = window.show();
            let _ = window.set_focus();
        }
    });
    Ok(true)
}

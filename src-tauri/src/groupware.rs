use crate::attendance::{
    groupware::{
        decode_cookie, detail_request, month_range, read_detail_rows, read_session_user,
        read_today_arrival, sign, today_request, DETAIL_PATH, ORIGIN, TODAY_PATH,
    },
    model::RawRow,
};
use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex, time::Duration};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};

const WINDOW_LABEL: &str = "groupware";

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

pub fn open_login_window(app: &AppHandle) -> Result<(), GroupwareError> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    let url: Url = format!("{ORIGIN}/#/login")
        .parse()
        .map_err(|error| failed(format!("그룹웨어 주소가 올바르지 않습니다: {error}")))?;
    WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(url))
        .title("그룹웨어 로그인")
        .inner_size(1100.0, 800.0)
        .build()
        .map(|_| ())
        .map_err(|error| failed(format!("로그인 창을 열 수 없습니다: {error}")))
}

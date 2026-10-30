use crate::attendance::model::RawRow;
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{Datelike, Months, NaiveDate, Weekday};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use sha2::Sha256;

pub const ORIGIN: &str = "https://gwa.soomgo.com";
pub const DETAIL_PATH: &str = "/human/hrd0410/selectTab2";
pub const TODAY_PATH: &str = "/human/common/judgeTimeManagement/getTodayComeLeaveInfo";

/// 그룹웨어 요청 서명: base64(HMAC-SHA256(signKey, 토큰 + transaction-id + timestamp + 경로))
pub fn sign(token: &str, transaction_id: &str, timestamp: &str, path: &str, sign_key: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(sign_key.as_bytes()).expect("HMAC accepts any key length");
    mac.update(token.as_bytes());
    mac.update(transaction_id.as_bytes());
    mac.update(timestamp.as_bytes());
    mac.update(path.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}

/// 쿠키 값의 퍼센트 인코딩(예: `%7C` → `|`)을 풉니다. 잘못된 시퀀스는 그대로 둡니다.
pub fn decode_cookie(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if let Some(byte) = value
                .get(index + 1..index + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                output.push(byte);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

pub fn month_range(today: NaiveDate) -> (NaiveDate, NaiveDate) {
    let first = today.with_day(1).expect("day 1 exists");
    let last = first
        .checked_add_months(Months::new(1))
        .and_then(|next| next.pred_opt())
        .expect("month end exists");
    (first, last)
}

pub fn detail_request(emp_code: &str, from: NaiveDate, to: NaiveDate) -> serde_json::Value {
    serde_json::json!({
        "admin": true,
        "appWorkTm": -1,
        "appWorkTmFlag": "1",
        "atDtFrom": from.format("%Y%m%d").to_string(),
        "atDtTo": to.format("%Y%m%d").to_string(),
        "empArr": [emp_code],
        "isIncludeHoliday": true,
        "searchType": "1",
    })
}

/// 로그인 창에서 읽은 세션 사용자 정보에서 꺼낸 본인 사번과 회사 코드
#[derive(Debug, PartialEq, Eq)]
pub struct SessionUser {
    pub emp_code: String,
    pub company_code: String,
}

/// 그룹웨어 sessionStorage의 `userInfo` JSON에서 본인 사번과 회사 코드를 읽습니다.
pub fn read_session_user(user_info: &str) -> Option<SessionUser> {
    let value: serde_json::Value = serde_json::from_str(user_info).ok()?;
    let text = |pointer: &str| {
        value.pointer(pointer).and_then(|field| match field {
            serde_json::Value::String(text) => Some(text.trim().to_owned()),
            serde_json::Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
        .filter(|text| !text.is_empty())
    };
    Some(SessionUser {
        emp_code: text("/ucUserInfo/erpEmpSeq").or_else(|| text("/erpUserInfo/userCode"))?,
        company_code: text("/ucUserInfo/compCd").or_else(|| text("/erpUserInfo/companyCode"))?,
    })
}

pub fn today_request(emp_code: &str, company_code: &str, today: NaiveDate) -> serde_json::Value {
    serde_json::json!({
        "empCd": emp_code,
        "coCd": company_code,
        "workDt": today.format("%Y%m%d").to_string(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TodayResponse {
    result_code: i64,
    result_data: Option<TodayData>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TodayData {
    come_tm: Option<String>,
}

/// 메인 화면의 오늘 출근 기록("YYYYMMDDHHMM")에서 오늘 출근 시각(분)을 읽습니다.
pub fn read_today_arrival(body: &[u8], today: NaiveDate) -> Option<i32> {
    let response: TodayResponse = serde_json::from_slice(body).ok()?;
    if response.result_code != 0 {
        return None;
    }
    let come = response.result_data?.come_tm?;
    let come = come.trim();
    if come.len() != 12 || come[..8] != today.format("%Y%m%d").to_string() {
        return None;
    }
    parse_hhmm(Some(&come[8..]), "", "").ok().flatten()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetailResponse {
    result_code: i64,
    result_msg: Option<String>,
    result_data: Option<Vec<DetailRow>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetailRow {
    at_dt: String,
    attresult_nm: Option<String>,
    at_nm: Option<String>,
    app_start_tm: Option<String>,
    app_end_tm: Option<String>,
    come_tm: Option<String>,
    leave_tm: Option<String>,
    outgowork_tm: Option<String>,
}

/// "0801" 형식(HHMM)을 분으로 바꿉니다. 빈 값은 None입니다.
fn parse_hhmm(value: Option<&str>, date: &str, field: &str) -> Result<Option<i32>, String> {
    let Some(text) = value.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    let invalid = || format!("{date} {field} 값을 읽을 수 없습니다: '{text}'");
    if text.len() != 4 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    let hour = text[..2].parse::<i32>().map_err(|_| invalid())?;
    let minute = text[2..].parse::<i32>().map_err(|_| invalid())?;
    if minute >= 60 {
        return Err(invalid());
    }
    Ok(Some(hour * 60 + minute))
}

fn weekday_label(date: NaiveDate) -> &'static str {
    match date.weekday() {
        Weekday::Mon => "월",
        Weekday::Tue => "화",
        Weekday::Wed => "수",
        Weekday::Thu => "목",
        Weekday::Fri => "금",
        Weekday::Sat => "토",
        Weekday::Sun => "일",
    }
}

pub fn read_detail_rows(body: &[u8]) -> Result<Vec<RawRow>, String> {
    let response: DetailResponse = serde_json::from_slice(body)
        .map_err(|error| format!("그룹웨어 응답 형식을 인식할 수 없습니다: {error}"))?;
    if response.result_code != 0 {
        return Err(format!(
            "그룹웨어 조회에 실패했습니다: {} (코드 {})",
            response.result_msg.unwrap_or_default(),
            response.result_code
        ));
    }
    let rows = response.result_data.unwrap_or_default();
    if rows.is_empty() {
        return Err("그룹웨어에서 받은 근무일 데이터가 없습니다. 사번을 확인하세요".to_owned());
    }
    rows.into_iter()
        .map(|row| {
            let date = NaiveDate::parse_from_str(&row.at_dt, "%Y%m%d")
                .map_err(|_| format!("근무일 값을 읽을 수 없습니다: '{}'", row.at_dt))?;
            let label = date.format("%Y-%m-%d").to_string();
            let outing_min = row
                .outgowork_tm
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(|text| {
                    text.parse::<i32>()
                        .ok()
                        .filter(|minutes| *minutes >= 0)
                        .ok_or_else(|| format!("{label} 외출 값을 읽을 수 없습니다: '{text}'"))
                })
                .transpose()?
                .filter(|minutes| *minutes > 0);
            Ok(RawRow {
                date,
                weekday: weekday_label(date).to_owned(),
                status: row.attresult_nm.unwrap_or_default().trim().to_owned(),
                item: row.at_nm.unwrap_or_default().trim().to_owned(),
                req_start: parse_hhmm(row.app_start_tm.as_deref(), &label, "근태신청 시작")?,
                req_end: parse_hhmm(row.app_end_tm.as_deref(), &label, "근태신청 종료")?,
                outing_min,
                actual_in: parse_hhmm(row.come_tm.as_deref(), &label, "실출근")?,
                actual_out: parse_hhmm(row.leave_tm.as_deref(), &label, "실퇴근")?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        decode_cookie, detail_request, month_range, read_detail_rows, read_session_user,
        read_today_arrival, sign, today_request, SessionUser,
    };
    use chrono::NaiveDate;

    #[test]
    fn signs_like_the_groupware_client() {
        assert_eq!(
            sign(
                "tok|1|abc",
                "0123456789abcdef0123456789abcdef",
                "1700000000",
                "/human/hrd0410/selectTab2",
                "12345"
            ),
            "GLqVq4/zQzwZ0akjjnN+E9z5/8MW6Qq/s1NAo4K6rSc="
        );
    }

    #[test]
    fn reads_today_arrival_only_for_today() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        let request = today_request("250000", "1000", today);
        assert_eq!(request["workDt"], "20261026");
        assert_eq!(request["coCd"], "1000");
        let body = br#"{"resultCode":0,"resultData":{"resultCode":"SUCCESS","comeTm":"202610260815","leaveTm":""}}"#;
        assert_eq!(read_today_arrival(body, today), Some(495));
        let yesterday = br#"{"resultCode":0,"resultData":{"comeTm":"202610250815"}}"#;
        assert_eq!(read_today_arrival(yesterday, today), None);
        let empty = br#"{"resultCode":0,"resultData":{"resultCode":"FAIL","comeTm":""}}"#;
        assert_eq!(read_today_arrival(empty, today), None);
    }

    #[test]
    fn reads_own_emp_code_from_session_user_info() {
        let user = read_session_user(
            r#"{"ucUserInfo":{"erpEmpSeq":"250000","compCd":"1000","loginId":"250000"},"erpUserInfo":{"userCode":"250000","companyCode":"1000"}}"#,
        );
        assert_eq!(
            user,
            Some(SessionUser { emp_code: "250000".to_owned(), company_code: "1000".to_owned() })
        );
        let fallback = read_session_user(r#"{"ucUserInfo":{},"erpUserInfo":{"userCode":"250001","companyCode":1000}}"#);
        assert_eq!(fallback.map(|user| user.emp_code), Some("250001".to_owned()));
        assert_eq!(read_session_user(r#"{"ucUserInfo":{"erpEmpSeq":""}}"#), None);
        assert_eq!(read_session_user("null"), None);
    }

    #[test]
    fn decodes_percent_encoded_cookie_values() {
        assert_eq!(decode_cookie("tok%7C1%7Cabc"), "tok|1|abc");
        assert_eq!(decode_cookie("plain"), "plain");
        assert_eq!(decode_cookie("bad%zz%7"), "bad%zz%7");
    }

    #[test]
    fn requests_the_whole_current_month() {
        let (from, to) = month_range(NaiveDate::from_ymd_opt(2026, 2, 14).unwrap());
        assert_eq!(from.to_string(), "2026-02-01");
        assert_eq!(to.to_string(), "2026-02-28");
        let body = detail_request("250000", from, to);
        assert_eq!(body["atDtFrom"], "20260201");
        assert_eq!(body["atDtTo"], "20260228");
        assert_eq!(body["empArr"][0], "250000");
    }

    #[test]
    fn maps_detail_rows_to_raw_rows() {
        let body = r#"{"resultCode":0,"resultMsg":"SUCCESS","resultData":[
            {"atDt":"20261001","attresultNm":"정상근무","atNm":"연차","appStartTm":"0900","appEndTm":"1800","comeTm":"","leaveTm":"","outgoworkTm":"0"},
            {"atDt":"20261003","attresultNm":"휴일","atNm":null,"appStartTm":null,"appEndTm":null,"comeTm":"","leaveTm":"","outgoworkTm":"0"},
            {"atDt":"20261006","attresultNm":"정상근무","atNm":null,"appStartTm":null,"appEndTm":null,"comeTm":"0751","leaveTm":"1716","outgoworkTm":"30"}
        ]}"#;
        let rows = read_detail_rows(body.as_bytes()).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].weekday, "목");
        assert_eq!(rows[0].item, "연차");
        assert_eq!(rows[0].req_start, Some(540));
        assert_eq!(rows[0].req_end, Some(1080));
        assert_eq!(rows[0].actual_in, None);
        assert_eq!(rows[0].outing_min, None);
        assert_eq!(rows[1].status, "휴일");
        assert_eq!(rows[1].item, "");
        assert_eq!(rows[2].actual_in, Some(471));
        assert_eq!(rows[2].actual_out, Some(1036));
        assert_eq!(rows[2].outing_min, Some(30));
    }

    #[test]
    fn reports_failed_or_empty_responses() {
        assert!(read_detail_rows(br#"{"resultCode":-1,"resultMsg":"denied"}"#)
            .unwrap_err()
            .contains("denied"));
        assert!(read_detail_rows(br#"{"resultCode":0,"resultData":[]}"#)
            .unwrap_err()
            .contains("사번"));
    }
}

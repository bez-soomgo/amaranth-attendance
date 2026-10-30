use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemRule {
    General,
    Bonus4h,
    Leave8h,
    DeductRequested,
    Exclude,
}

impl ItemRule {
    pub fn label(&self) -> &'static str {
        match self {
            Self::General => "일반 근무",
            Self::Bonus4h => "4시간 가산",
            Self::Leave8h => "8시간 인정(연차)",
            Self::DeductRequested => "근태신청 시간만큼 추가 차감",
            Self::Exclude => "계산 제외",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RawRow {
    pub date: NaiveDate,
    pub weekday: String,
    pub status: String,
    pub item: String,
    pub req_start: Option<i32>,
    pub req_end: Option<i32>,
    pub outing_min: Option<i32>,
    pub actual_in: Option<i32>,
    pub actual_out: Option<i32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DayKind {
    Work,
    Leave,
    InProgress,
    Incomplete,
    Excluded,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DayRow {
    pub date: String,
    pub weekday: String,
    pub item: String,
    pub kind: DayKind,
    pub actual_in: Option<String>,
    pub recognized_in: Option<String>,
    pub out: Option<String>,
    pub worked_min: Option<i32>,
    pub break_min: i32,
    pub bonus_min: i32,
    pub final_min: Option<i32>,
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub work_days: u32,
    pub actual_days: u32,
    pub leave_days: u32,
    pub target_min: i32,
    pub total_min: i32,
    pub diff_min: i32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub period: String,
    pub rows: Vec<DayRow>,
    pub summary: Summary,
    pub notes: Vec<String>,
    pub in_progress_arrival: Option<String>,
    /// 오늘 근무 중인 날의 근태항목이 4시간 가산 대상인지 여부
    pub in_progress_bonus: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Analysis {
    NeedsRules { unknown_items: Vec<String> },
    Ready { report: Report },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TodayPlan {
    pub needed_min: i32,
    pub work_needed_min: i32,
    pub recognized_in: String,
    pub leave_at: Option<String>,
    pub break_applied: bool,
    pub already_met: bool,
    pub half_day_notice: bool,
}

#[cfg(test)]
mod tests {
    use super::Analysis;

    #[test]
    fn needs_rules_uses_camel_case_json_fields() {
        let value = serde_json::to_value(Analysis::NeedsRules {
            unknown_items: vec!["재택근무".to_owned()],
        })
        .unwrap();
        assert_eq!(value["status"], "needsRules");
        assert_eq!(value["unknownItems"][0], "재택근무");
    }
}

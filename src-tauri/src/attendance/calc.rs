use crate::attendance::{
    model::{Analysis, DayKind, DayRow, ItemRule, RawRow, Report, Summary},
    rules::{builtin_rule, norm, resolve},
    time::{fmt_clock, fmt_hm},
};
use chrono::NaiveDate;
use std::collections::{BTreeMap, HashSet};

const STANDARD_NOTES: [&str; 6] = [
    "인정출근: 08:00 이전 출근은 08:00으로 조정",
    "휴게: 퇴근이 14:00 이후면 1시간 차감 (14:00 이전 퇴근은 차감 없음)",
    "4시간 가산: 오전반차·오후반차·생일휴가·단축(명절, 연휴 마지막날)·건강검진 (휴게 차감은 일반 근무와 동일)",
    "연차 또는 출퇴근 기록 없음: 8시간 인정 (근무일 포함)",
    "업무 중 휴게: 근태신청 시간만큼 추가 차감",
    "기준 근무시간: 근무일수 × 8시간",
];

fn time_text(minutes: Option<i32>) -> Option<String> {
    minutes.map(fmt_clock)
}

fn custom_rule_is_used(normalized: &str, custom: &BTreeMap<String, ItemRule>) -> bool {
    builtin_rule(normalized).is_none() && custom.contains_key(normalized)
}

fn append_note(note: &mut String, extra: String) {
    if note.is_empty() {
        *note = extra;
    } else {
        note.push_str(" · ");
        note.push_str(&extra);
    }
}

fn custom_note(rule: &ItemRule) -> String {
    format!("사용자 지정: {}", rule.label())
}

fn period(rows: &[RawRow]) -> String {
    let Some(minimum) = rows.iter().map(|row| row.date).min() else {
        return String::new();
    };
    let maximum = rows.iter().map(|row| row.date).max().unwrap_or(minimum);
    let first = minimum.format("%Y-%m").to_string();
    let last = maximum.format("%Y-%m").to_string();
    if first == last {
        first
    } else {
        format!("{first} ~ {last}")
    }
}

pub fn analyze(rows: &[RawRow], custom: &BTreeMap<String, ItemRule>, today: NaiveDate) -> Analysis {
    let mut unknown_items = Vec::new();
    let mut unknown_norms = HashSet::new();
    let mut resolved = Vec::new();
    for row in rows {
        if row.status != "정상근무" || row.date > today {
            continue;
        }
        let normalized = norm(&row.item);
        match resolve(&normalized, custom) {
            Some(rule) => resolved.push((row, normalized, rule)),
            None if unknown_norms.insert(normalized) => {
                unknown_items.push(row.item.trim().to_owned());
            }
            None => {}
        }
    }
    if !unknown_items.is_empty() {
        return Analysis::NeedsRules { unknown_items };
    }

    let mut day_rows = Vec::with_capacity(resolved.len());
    let mut actual_days = 0_u32;
    let mut leave_days = 0_u32;
    let mut total_min = 0_i32;
    let mut in_progress_arrival = None;
    let mut custom_notes = Vec::new();
    let mut noted_custom_items = HashSet::new();

    for (row, normalized, rule) in resolved {
        let is_custom = custom_rule_is_used(&normalized, custom);
        if is_custom && noted_custom_items.insert(normalized.clone()) {
            custom_notes.push(format!(
                "사용자 지정 '{}': {}",
                row.item.trim(),
                rule.label()
            ));
        }
        let actual_in = time_text(row.actual_in);
        let out_text = time_text(row.actual_out);
        let recognized_in = row.actual_in.map(|minutes| fmt_clock(minutes.max(480)));
        let both_blank = row.actual_in.is_none() && row.actual_out.is_none();
        let custom_day_note = || custom_note(&rule);

        if rule == ItemRule::Exclude {
            day_rows.push(DayRow {
                date: row.date.format("%Y-%m-%d").to_string(),
                weekday: row.weekday.clone(),
                item: row.item.trim().to_owned(),
                kind: DayKind::Excluded,
                actual_in,
                recognized_in,
                out: out_text,
                worked_min: None,
                break_min: 0,
                bonus_min: 0,
                final_min: None,
                note: "계산 제외(사용자 지정)".to_owned(),
            });
            continue;
        }

        if rule == ItemRule::Leave8h || both_blank {
            leave_days += 1;
            total_min += 480;
            let mut note = if normalized == "연차" {
                "연차 8시간".to_owned()
            } else {
                "출퇴근 기록 없음 → 연차 8시간".to_owned()
            };
            if is_custom {
                append_note(&mut note, custom_day_note());
            }
            day_rows.push(DayRow {
                date: row.date.format("%Y-%m-%d").to_string(),
                weekday: row.weekday.clone(),
                item: row.item.trim().to_owned(),
                kind: DayKind::Leave,
                actual_in: None,
                recognized_in: None,
                out: None,
                worked_min: None,
                break_min: 0,
                bonus_min: 0,
                final_min: Some(480),
                note,
            });
            continue;
        }

        if row.actual_in.is_none() || row.actual_out.is_none() {
            let in_progress = row.date == today && row.actual_in.is_some();
            let kind = if in_progress {
                DayKind::InProgress
            } else {
                DayKind::Incomplete
            };
            let note = if in_progress {
                in_progress_arrival = row.actual_in.map(fmt_clock);
                "오늘 근무 중 — 계산 제외"
            } else {
                "출퇴근 기록 불완전 — 계산 제외"
            };
            day_rows.push(DayRow {
                date: row.date.format("%Y-%m-%d").to_string(),
                weekday: row.weekday.clone(),
                item: row.item.trim().to_owned(),
                kind,
                actual_in,
                recognized_in,
                out: out_text,
                worked_min: None,
                break_min: 0,
                bonus_min: 0,
                final_min: None,
                note: note.to_owned(),
            });
            continue;
        }

        let actual_in_min = row.actual_in.expect("complete rows have an arrival time");
        let mut out_min = row.actual_out.expect("complete rows have a departure time");
        if out_min < actual_in_min {
            out_min += 1440;
        }
        let recognized_min = actual_in_min.max(480);
        let worked_min = (out_min - recognized_min).max(0);
        let break_min = if out_min > 840 { 60 } else { 0 };
        let mut additional_break = 0;
        let mut missing_request_note = false;
        if rule == ItemRule::DeductRequested {
            if let (Some(start), Some(end)) = (row.req_start, row.req_end) {
                additional_break = end - start;
            } else {
                missing_request_note = true;
            }
        }
        let break_min = break_min + additional_break;
        let bonus_min = if rule == ItemRule::Bonus4h { 240 } else { 0 };
        let final_min = worked_min - break_min + bonus_min;
        total_min += final_min;
        actual_days += 1;

        let mut note = String::new();
        if rule == ItemRule::DeductRequested {
            if missing_request_note {
                append_note(&mut note, "근태신청 시간 없음 — 추가 차감 0".to_owned());
            } else {
                append_note(
                    &mut note,
                    format!("업무 중 휴게 {} 추가 차감", fmt_hm(additional_break)),
                );
            }
        }
        if is_custom {
            append_note(&mut note, custom_day_note());
        }
        day_rows.push(DayRow {
            date: row.date.format("%Y-%m-%d").to_string(),
            weekday: row.weekday.clone(),
            item: row.item.trim().to_owned(),
            kind: DayKind::Work,
            actual_in,
            recognized_in: Some(fmt_clock(recognized_min)),
            out: Some(fmt_clock(out_min)),
            worked_min: Some(worked_min),
            break_min,
            bonus_min,
            final_min: Some(final_min),
            note,
        });
    }

    let work_days = actual_days + leave_days;
    let target_min = work_days as i32 * 480;
    let mut notes = STANDARD_NOTES
        .iter()
        .map(|note| (*note).to_owned())
        .collect::<Vec<_>>();
    notes.extend(custom_notes);
    Analysis::Ready {
        report: Report {
            period: period(rows),
            rows: day_rows,
            summary: Summary {
                work_days,
                actual_days,
                leave_days,
                target_min,
                total_min,
                diff_min: total_min - target_min,
            },
            notes,
            in_progress_arrival,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::analyze;
    use crate::attendance::model::{Analysis, DayKind, ItemRule, RawRow};
    use chrono::NaiveDate;
    use std::collections::BTreeMap;

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
    }

    fn row(day: u32, item: &str, actual_in: Option<i32>, actual_out: Option<i32>) -> RawRow {
        RawRow {
            date: date(day),
            weekday: "금".to_owned(),
            status: "정상근무".to_owned(),
            item: item.to_owned(),
            req_start: None,
            req_end: None,
            actual_in,
            actual_out,
        }
    }

    fn ready(
        rows: &[RawRow],
        custom: &BTreeMap<String, ItemRule>,
    ) -> crate::attendance::model::Report {
        match analyze(rows, custom, date(2)) {
            Analysis::Ready { report } => report,
            Analysis::NeedsRules { unknown_items } => panic!("unexpected rules: {unknown_items:?}"),
        }
    }

    #[test]
    fn break_starts_after_two_pm_and_arrival_before_eight_is_adjusted() {
        let rows = [
            row(1, "", Some(480), Some(840)),
            row(2, "", Some(470), Some(841)),
        ];
        let report = ready(&rows, &BTreeMap::new());
        assert_eq!(report.rows[0].break_min, 0);
        assert_eq!(report.rows[1].break_min, 60);
        assert_eq!(report.rows[1].recognized_in.as_deref(), Some("08:00"));
    }

    #[test]
    fn half_day_aliases_and_health_check_add_four_hours() {
        let rows = [
            row(1, "단축(명절,연마지막날)", Some(480), Some(840)),
            row(2, "단축(명절, 연휴 마지막날)", Some(480), Some(840)),
            row(2, "건강검진", Some(611), Some(781)),
        ];
        let report = ready(&rows, &BTreeMap::new());
        assert_eq!(report.rows[0].bonus_min, 240);
        assert_eq!(report.rows[1].bonus_min, 240);
        assert_eq!(report.rows[2].final_min, Some(410));
    }

    #[test]
    fn blank_half_day_is_leave_and_requested_break_is_deducted() {
        let mut half_day = row(1, "오전반차", None, None);
        let mut break_row = row(2, "업무 중 휴게", Some(503), Some(1193));
        break_row.req_start = Some(1110);
        break_row.req_end = Some(1170);
        let report = ready(&[half_day.clone(), break_row], &BTreeMap::new());
        assert_eq!(report.rows[0].kind, DayKind::Leave);
        assert_eq!(report.rows[0].final_min, Some(480));
        assert_eq!(report.rows[1].final_min, Some(570));
        half_day.item = "연차".to_owned();
        let leave_report = ready(&[half_day], &BTreeMap::new());
        assert_eq!(leave_report.rows[0].note, "연차 8시간");
    }

    #[test]
    fn unknown_custom_excluded_and_future_rows_follow_rules() {
        let unknown = row(1, "재택근무", Some(540), Some(1080));
        assert!(matches!(
            analyze(&[unknown.clone()], &BTreeMap::new(), date(2)),
            Analysis::NeedsRules { unknown_items } if unknown_items == ["재택근무"]
        ));
        let custom = BTreeMap::from([("재택근무".to_owned(), ItemRule::Exclude)]);
        let report = ready(&[unknown], &custom);
        assert_eq!(report.rows[0].kind, DayKind::Excluded);
        assert_eq!(report.summary.work_days, 0);
        let future = row(3, "", Some(480), Some(960));
        assert!(ready(&[future], &BTreeMap::new()).rows.is_empty());
    }

    #[test]
    fn today_with_arrival_and_no_departure_is_in_progress() {
        let report = ready(&[row(2, "", Some(510), None)], &BTreeMap::new());
        assert_eq!(report.rows[0].kind, DayKind::InProgress);
        assert_eq!(report.in_progress_arrival.as_deref(), Some("08:30"));
        assert_eq!(report.summary.work_days, 0);
    }
}

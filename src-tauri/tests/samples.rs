use amaranth_attendance_lib::attendance::{
    calc::analyze,
    export::write_report,
    model::{Analysis, ItemRule},
    parse::read_rows,
};
use calamine::{Data, Reader, Xlsx};
use chrono::NaiveDate;
use std::{collections::BTreeMap, env, fs, io::Cursor, path::PathBuf};

#[test]
#[ignore = "requires private attendance exports"]
fn provided_exports_match_expected_summaries_and_export() {
    let Some(sample_dir) = env::var_os("AMARANTH_SAMPLES_DIR").map(PathBuf::from) else {
        eprintln!("AMARANTH_SAMPLES_DIR is unset; skipping sample assertions");
        return;
    };
    let names = [
        "detailGrid_excel_2026-10-02_11-27.xlsx",
        "detailGrid_excel_2026-09-29_10-48.xlsx",
        "detailGrid_excel_2026-02-27_15-15.xlsx",
        "detailGrid_excel_2026-06-30_16-57.xlsx",
        "detailGrid_excel_2026-05-29_08-49.xlsx",
        "detailGrid_excel_2026-06-29_16-38.xlsx",
    ];
    if names.iter().any(|name| !sample_dir.join(name).is_file()) {
        eprintln!(
            "One or more sample exports are missing from {}; skipping sample assertions",
            sample_dir.display()
        );
        return;
    }

    let today = NaiveDate::from_ymd_opt(2026, 10, 2).unwrap();
    let custom = BTreeMap::<String, ItemRule>::new();
    let expected = [
        Some((20, 2, 9_678, 78)),
        None,
        Some((16, 0, 7_854, 174)),
        Some((20, 5, 9_627, 27)),
        Some((17, 0, 8_239, 79)),
        Some((19, 0, 8_947, -173)),
    ];
    let mut october_report = None;

    for (index, name) in names.iter().enumerate() {
        let bytes = fs::read(sample_dir.join(name)).unwrap();
        let rows = read_rows(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        let Analysis::Ready { report } = analyze(&rows, &custom, today) else {
            panic!("{name}: expected Ready analysis");
        };
        if let Some((work_days, leave_days, total_min, diff_min)) = expected[index] {
            assert_eq!(report.summary.work_days, work_days, "{name}");
            assert_eq!(report.summary.leave_days, leave_days, "{name}");
            assert_eq!(report.summary.total_min, total_min, "{name}");
            assert_eq!(report.summary.diff_min, diff_min, "{name}");
        }
        println!(
            "{name}: work_days={}, leave_days={}, total_min={}, diff_min={}",
            report.summary.work_days,
            report.summary.leave_days,
            report.summary.total_min,
            report.summary.diff_min
        );
        if index == 0 {
            october_report = Some(report);
        }
    }

    let report = october_report.expect("October report was captured");
    let output = env::temp_dir().join(format!("amaranth-sample-{}.xlsx", std::process::id()));
    write_report(&report, &output).unwrap();
    let exported = fs::read(&output).unwrap();
    let _ = fs::remove_file(&output);
    let mut workbook = Xlsx::new(Cursor::new(exported)).unwrap();
    let sheet = workbook.worksheet_range_at(0).unwrap().unwrap();
    let mut has_total = false;
    let mut exported_total = None;
    for row in sheet.rows() {
        for (column, value) in row.iter().enumerate() {
            if value == &Data::String("합계".to_owned()) {
                has_total = true;
            }
            if value == &Data::String("실 근무시간 합계".to_owned()) {
                exported_total = row.get(column + 1).and_then(|value| match value {
                    Data::Float(number) => Some(*number),
                    Data::Int(number) => Some(*number as f64),
                    Data::DateTime(number) => Some(number.as_f64()),
                    _ => None,
                });
            }
        }
    }
    assert!(has_total, "export must contain a 합계 row");
    let exported_total = exported_total.expect("export must include 실 근무시간 합계");
    assert!((exported_total - 9_678.0 / 1_440.0).abs() < 0.000_001);
}

#[test]
#[ignore = "requires the private 2026-10-13 attendance export"]
fn subtracts_recorded_outing_time_from_worked_time() {
    let Some(path) = env::var_os("AMARANTH_BREAK_SAMPLE_PATH").map(PathBuf::from) else {
        eprintln!("AMARANTH_BREAK_SAMPLE_PATH is unset; skipping break-time assertion");
        return;
    };
    if !path.is_file() {
        eprintln!(
            "Break-time sample is missing at {}; skipping assertion",
            path.display()
        );
        return;
    }
    let bytes = fs::read(&path).unwrap();
    let rows = read_rows(&bytes).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
    let Analysis::Ready { report } = analyze(&rows, &BTreeMap::new(), today) else {
        panic!("expected the supplied workbook to produce a report");
    };
    let row = report
        .rows
        .iter()
        .find(|row| row.date == "2026-10-06")
        .expect("the recorded-rest sample row should be present");
    assert_eq!(
        row.break_min, 60,
        "the standard lunch break remains 60 minutes"
    );
    assert_eq!(row.final_min, Some(466), "Excel's basic work time is 7:46");
    assert_eq!(row.note, "외출 0:30 추가 차감");
    let output = env::temp_dir().join(format!("amaranth-outing-{}.xlsx", std::process::id()));
    write_report(&report, &output).unwrap();
    let exported = fs::read(&output).unwrap();
    let _ = fs::remove_file(&output);
    let mut workbook = Xlsx::new(Cursor::new(exported)).unwrap();
    let sheet = workbook.worksheet_range_at(0).unwrap().unwrap();
    let exported_row = sheet
        .rows()
        .find(|cells| cells.first() == Some(&Data::String("2026-10-06".to_owned())))
        .expect("the exported report should include the affected date");
    let exported_final = match exported_row.get(9) {
        Some(Data::DateTime(value)) => (value.as_f64() * 1440.0).round() as i32,
        Some(Data::Float(value)) => (*value * 1440.0).round() as i32,
        _ => panic!("exported final work time should be numeric"),
    };
    assert_eq!(exported_final, 466);
    assert_eq!(
        exported_row.get(10),
        Some(&Data::String("외출 0:30 추가 차감".to_owned()))
    );
}

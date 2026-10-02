use crate::attendance::{
    model::{DayKind, DayRow, Report},
    time::fmt_hm,
};
use rust_xlsxwriter::{Format, Workbook, Worksheet};
use std::path::Path;

fn write_duration(
    worksheet: &mut Worksheet,
    row: u32,
    column: u16,
    minutes: Option<i32>,
    format: &Format,
) -> Result<(), rust_xlsxwriter::XlsxError> {
    if let Some(minutes) = minutes {
        worksheet.write_with_format(row, column, minutes as f64 / 1440.0, format)?;
    }
    Ok(())
}

fn is_counted(row: &DayRow) -> bool {
    matches!(row.kind, DayKind::Work | DayKind::Leave)
}

fn signed_diff(minutes: i32) -> String {
    match minutes.cmp(&0) {
        std::cmp::Ordering::Greater => format!("+{} (초과)", fmt_hm(minutes)),
        std::cmp::Ordering::Less => format!("{} (부족)", fmt_hm(minutes)),
        std::cmp::Ordering::Equal => "0:00".to_owned(),
    }
}

pub fn write_report(report: &Report, out: &Path) -> Result<(), String> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet
        .set_name("근무시간")
        .map_err(|error| format!("엑셀 시트 이름을 설정할 수 없습니다: {error}"))?;
    let bold = Format::new().set_bold();
    let duration = Format::new().set_num_format("[h]:mm");

    worksheet
        .write_with_format(0, 0, format!("{} 근무시간", report.period), &bold)
        .map_err(|error| format!("엑셀 제목을 쓸 수 없습니다: {error}"))?;
    let headers = [
        "날짜",
        "요일",
        "근태항목",
        "실출근",
        "인정출근",
        "퇴근",
        "인정근무(휴게 전)",
        "휴게차감",
        "반차가산",
        "최종근무시간",
        "비고",
    ];
    for (column, header) in headers.iter().enumerate() {
        worksheet
            .write_with_format(2, column as u16, *header, &bold)
            .map_err(|error| format!("엑셀 머리글을 쓸 수 없습니다: {error}"))?;
    }

    for (index, row) in report.rows.iter().enumerate() {
        let excel_row = index as u32 + 3;
        worksheet
            .write(excel_row, 0, &row.date)
            .map_err(|error| format!("엑셀 날짜를 쓸 수 없습니다: {error}"))?;
        worksheet
            .write(excel_row, 1, &row.weekday)
            .map_err(|error| format!("엑셀 요일을 쓸 수 없습니다: {error}"))?;
        worksheet
            .write(excel_row, 2, &row.item)
            .map_err(|error| format!("엑셀 근태항목을 쓸 수 없습니다: {error}"))?;
        for (column, value) in [
            (3, row.actual_in.as_deref().unwrap_or("-")),
            (4, row.recognized_in.as_deref().unwrap_or("-")),
            (5, row.out.as_deref().unwrap_or("-")),
        ] {
            worksheet
                .write(excel_row, column, value)
                .map_err(|error| format!("엑셀 출퇴근 시각을 쓸 수 없습니다: {error}"))?;
        }
        write_duration(worksheet, excel_row, 6, row.worked_min, &duration)
            .map_err(|error| format!("엑셀 인정근무시간을 쓸 수 없습니다: {error}"))?;
        write_duration(worksheet, excel_row, 7, Some(row.break_min), &duration)
            .map_err(|error| format!("엑셀 휴게차감을 쓸 수 없습니다: {error}"))?;
        write_duration(worksheet, excel_row, 8, Some(row.bonus_min), &duration)
            .map_err(|error| format!("엑셀 가산시간을 쓸 수 없습니다: {error}"))?;
        match row.kind {
            DayKind::InProgress | DayKind::Incomplete | DayKind::Excluded => {
                worksheet
                    .write(excel_row, 9, "제외")
                    .map_err(|error| format!("엑셀 제외 표시를 쓸 수 없습니다: {error}"))?;
            }
            DayKind::Work | DayKind::Leave => {
                write_duration(worksheet, excel_row, 9, row.final_min, &duration)
                    .map_err(|error| format!("엑셀 최종근무시간을 쓸 수 없습니다: {error}"))?;
            }
        }
        worksheet
            .write(excel_row, 10, &row.note)
            .map_err(|error| format!("엑셀 비고를 쓸 수 없습니다: {error}"))?;
    }

    let total_row = report.rows.len() as u32 + 3;
    worksheet
        .write_with_format(total_row, 0, "합계", &bold)
        .map_err(|error| format!("엑셀 합계 행을 쓸 수 없습니다: {error}"))?;
    let counted_rows = report.rows.iter().filter(|row| is_counted(row));
    let worked_total = counted_rows.clone().filter_map(|row| row.worked_min).sum();
    let break_total = counted_rows.clone().map(|row| row.break_min).sum();
    let bonus_total = counted_rows.clone().map(|row| row.bonus_min).sum();
    let final_total = counted_rows.filter_map(|row| row.final_min).sum();
    for (column, minutes) in [
        (6, worked_total),
        (7, break_total),
        (8, bonus_total),
        (9, final_total),
    ] {
        write_duration(worksheet, total_row, column, Some(minutes), &duration)
            .map_err(|error| format!("엑셀 합계를 쓸 수 없습니다: {error}"))?;
    }

    let summary_row = total_row + 2;
    worksheet
        .write_with_format(summary_row, 0, "요약", &bold)
        .map_err(|error| format!("엑셀 요약 제목을 쓸 수 없습니다: {error}"))?;
    let work_days = format!(
        "{}일 (실근무 {}일 + 연차 {}일)",
        report.summary.work_days, report.summary.actual_days, report.summary.leave_days
    );
    for (offset, (label, value)) in [
        ("근무일수", work_days),
        ("기준 근무시간", String::new()),
        ("실 근무시간 합계", String::new()),
        ("부족/초과", signed_diff(report.summary.diff_min)),
    ]
    .into_iter()
    .enumerate()
    {
        let row = summary_row + offset as u32 + 1;
        worksheet
            .write(row, 0, label)
            .map_err(|error| format!("엑셀 요약 항목을 쓸 수 없습니다: {error}"))?;
        if offset < 3 {
            if offset == 0 {
                worksheet
                    .write(row, 1, value)
                    .map_err(|error| format!("엑셀 근무일수를 쓸 수 없습니다: {error}"))?;
            } else {
                let minutes = if offset == 1 {
                    report.summary.target_min
                } else {
                    report.summary.total_min
                };
                write_duration(worksheet, row, 1, Some(minutes), &duration)
                    .map_err(|error| format!("엑셀 요약 시간을 쓸 수 없습니다: {error}"))?;
            }
        } else {
            worksheet
                .write(row, 1, value)
                .map_err(|error| format!("엑셀 부족/초과를 쓸 수 없습니다: {error}"))?;
        }
    }

    let notes_header = summary_row + 6;
    worksheet
        .write_with_format(notes_header, 0, "비고", &bold)
        .map_err(|error| format!("엑셀 비고 제목을 쓸 수 없습니다: {error}"))?;
    for (index, note) in report.notes.iter().enumerate() {
        worksheet
            .write(notes_header + index as u32 + 1, 0, note)
            .map_err(|error| format!("엑셀 비고를 쓸 수 없습니다: {error}"))?;
    }

    for (column, width) in [
        (0, 12.0),
        (1, 8.0),
        (2, 18.0),
        (3, 14.0),
        (4, 14.0),
        (5, 14.0),
        (6, 14.0),
        (7, 14.0),
        (8, 14.0),
        (9, 14.0),
        (10, 40.0),
    ] {
        worksheet
            .set_column_width(column, width)
            .map_err(|error| format!("엑셀 열 너비를 설정할 수 없습니다: {error}"))?;
    }
    workbook
        .save(out)
        .map_err(|error| format!("엑셀 파일을 저장할 수 없습니다: {error}"))
}

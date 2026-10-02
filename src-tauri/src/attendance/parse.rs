use crate::attendance::{model::RawRow, time::parse_hm};
use calamine::{Data, Range, Reader, Xlsx};
use chrono::NaiveDate;
use std::io::{Cursor, Read, Write};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

const REPAIRED_STYLES: &[u8] = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="0"/><fonts count="1"><font/></fonts><fills count="1"><fill/></fills><borders count="1"><border/></borders><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellXfs></styleSheet>"#;

#[derive(Clone, Copy)]
enum ZipRepair {
    ReplaceStyles,
    StripStyleAttributes,
}

fn read_range(bytes: &[u8]) -> Result<Range<Data>, String> {
    let mut workbook = Xlsx::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
    workbook
        .worksheet_range_at(0)
        .ok_or_else(|| "첫 번째 시트를 찾을 수 없습니다".to_owned())?
        .map_err(|error| error.to_string())
}

fn rebuild_zip(bytes: &[u8], repair: ZipRepair) -> Result<Vec<u8>, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let mut found_styles = false;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry.name().to_owned();
        if name == "xl/styles.xml" {
            found_styles = true;
        }
        let options = SimpleFileOptions::default();
        if entry.is_dir() {
            writer
                .add_directory(&name, options)
                .map_err(|error| error.to_string())?;
            continue;
        }
        writer
            .start_file(&name, options)
            .map_err(|error| error.to_string())?;
        match repair {
            ZipRepair::ReplaceStyles if name == "xl/styles.xml" => writer
                .write_all(REPAIRED_STYLES)
                .map_err(|error| error.to_string())?,
            ZipRepair::StripStyleAttributes
                if name.starts_with("xl/worksheets/") && name.ends_with(".xml") =>
            {
                let mut xml = Vec::new();
                entry
                    .read_to_end(&mut xml)
                    .map_err(|error| error.to_string())?;
                writer
                    .write_all(&strip_style_attributes(&xml))
                    .map_err(|error| error.to_string())?;
            }
            _ => {
                std::io::copy(&mut entry, &mut writer).map_err(|error| error.to_string())?;
            }
        }
    }

    if matches!(repair, ZipRepair::ReplaceStyles) && !found_styles {
        writer
            .start_file("xl/styles.xml", SimpleFileOptions::default())
            .map_err(|error| error.to_string())?;
        writer
            .write_all(REPAIRED_STYLES)
            .map_err(|error| error.to_string())?;
    }
    writer
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| error.to_string())
}

fn strip_style_attributes(xml: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(xml.len());
    let mut index = 0;
    while index < xml.len() {
        if xml[index..].starts_with(b" s=\"") {
            let digits_start = index + 4;
            let mut end = digits_start;
            while end < xml.len() && xml[end].is_ascii_digit() {
                end += 1;
            }
            if end > digits_start && xml.get(end) == Some(&b'\"') {
                index = end + 1;
                continue;
            }
        }
        output.push(xml[index]);
        index += 1;
    }
    output
}

fn cell_text(cell: Option<&Data>) -> Option<String> {
    match cell? {
        Data::Empty => None,
        Data::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        value => {
            let text = value.to_string();
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
    }
}

fn parse_time(cell: Option<&Data>, date: &str, column_name: &str) -> Result<Option<i32>, String> {
    let Some(value) = cell else {
        return Ok(None);
    };
    let parsed = match value {
        Data::Empty => return Ok(None),
        Data::String(text) if text.trim().is_empty() => return Ok(None),
        Data::String(text) => parse_hm(text.trim()),
        Data::Float(day_fraction) => Some((day_fraction * 1440.0).round() as i32),
        Data::DateTime(date_time) => Some((date_time.as_f64() * 1440.0).round() as i32),
        _ => None,
    };
    parsed.map(Some).ok_or_else(|| {
        format!(
            "{date} {column_name} 값을 읽을 수 없습니다: '{}'",
            value.to_string().trim()
        )
    })
}

fn column_index(header: &[Data], name: &str) -> Option<usize> {
    header
        .iter()
        .position(|cell| cell_text(Some(cell)).as_deref() == Some(name))
}

pub fn read_rows(bytes: &[u8]) -> Result<Vec<RawRow>, String> {
    let range = match read_range(bytes) {
        Ok(range) => range,
        Err(_) => {
            let repaired = rebuild_zip(bytes, ZipRepair::ReplaceStyles)
                .map_err(|error| format!("엑셀 파일을 읽을 수 없습니다: {error}"))?;
            match read_range(&repaired) {
                Ok(range) => range,
                Err(_) => {
                    let repaired = rebuild_zip(&repaired, ZipRepair::StripStyleAttributes)
                        .map_err(|error| format!("엑셀 파일을 읽을 수 없습니다: {error}"))?;
                    read_range(&repaired)
                        .map_err(|error| format!("엑셀 파일을 읽을 수 없습니다: {error}"))?
                }
            }
        }
    };

    let rows = range.rows().collect::<Vec<_>>();
    let header_row = rows
        .iter()
        .position(|row| column_index(row, "근무일").is_some())
        .ok_or_else(|| "엑셀 형식을 인식할 수 없습니다: '근무일' 열이 없습니다".to_owned())?;
    let header = rows[header_row];
    let require_column = |name: &str| {
        column_index(header, name)
            .ok_or_else(|| format!("엑셀 형식을 인식할 수 없습니다: '{name}' 열이 없습니다"))
    };
    let date_col = require_column("근무일")?;
    let weekday_col = require_column("요일")?;
    let status_col = require_column("출결구분")?;
    let item_col = require_column("근태항목")?;
    let req_start_col = require_column("근태신청")?;
    let actual_in_col = require_column("실출퇴근시각")?;
    let req_end_col = req_start_col + 1;
    let actual_out_col = actual_in_col + 1;

    let mut result = Vec::new();
    for row in rows.iter().skip(header_row + 1) {
        let Some(date_text) = row.get(date_col).and_then(|cell| cell_text(Some(cell))) else {
            continue;
        };
        let Ok(date) = NaiveDate::parse_from_str(&date_text, "%Y-%m-%d") else {
            continue;
        };
        let date_label = date.format("%Y-%m-%d").to_string();
        let value_at = |column: usize| row.get(column);
        result.push(RawRow {
            date,
            weekday: value_at(weekday_col)
                .and_then(|cell| cell_text(Some(cell)))
                .unwrap_or_default(),
            status: value_at(status_col)
                .and_then(|cell| cell_text(Some(cell)))
                .unwrap_or_default(),
            item: value_at(item_col)
                .and_then(|cell| cell_text(Some(cell)))
                .unwrap_or_default(),
            req_start: parse_time(value_at(req_start_col), &date_label, "근태신청 시작")?,
            req_end: parse_time(value_at(req_end_col), &date_label, "근태신청 종료")?,
            actual_in: parse_time(value_at(actual_in_col), &date_label, "실출근")?,
            actual_out: parse_time(value_at(actual_out_col), &date_label, "실퇴근")?,
        });
    }
    if result.is_empty() {
        return Err("근무일 데이터가 없습니다".to_owned());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::read_rows;
    use rust_xlsxwriter::Workbook;

    #[test]
    fn parses_sample_layout_by_header_names_and_string_times() {
        let mut workbook = Workbook::new();
        let sheet = workbook.add_worksheet();
        for (column, title) in [
            (0, "No"),
            (1, "근무일"),
            (2, "요일"),
            (3, "근무유형"),
            (4, "출결구분"),
            (5, "근태항목"),
            (6, "상태"),
            (7, "정상근무범위"),
            (9, "근태신청"),
            (11, "실출퇴근시각"),
        ] {
            sheet.write_string(0, column, title).unwrap();
        }
        sheet.write_string(1, 9, "시작").unwrap();
        sheet.write_string(1, 10, "종료").unwrap();
        sheet.write_string(1, 11, "출근").unwrap();
        sheet.write_string(1, 12, "퇴근").unwrap();
        let data = [
            ["2026-10-01", "목", "정상근무", "", "", "", "08:05", "17:01"],
            ["2026-10-02", "금", "정상근무", "오전반차", "", "", "", ""],
            ["2026-10-03", "토", "휴일", "", "", "", "", ""],
        ];
        for (row_index, row_data) in data.iter().enumerate() {
            let row = row_index as u32 + 3;
            sheet.write_string(row, 1, row_data[0]).unwrap();
            sheet.write_string(row, 2, row_data[1]).unwrap();
            sheet.write_string(row, 4, row_data[2]).unwrap();
            sheet.write_string(row, 5, row_data[3]).unwrap();
            sheet.write_string(row, 9, row_data[4]).unwrap();
            sheet.write_string(row, 10, row_data[5]).unwrap();
            sheet.write_string(row, 11, row_data[6]).unwrap();
            sheet.write_string(row, 12, row_data[7]).unwrap();
        }
        let bytes = workbook.save_to_buffer().unwrap();

        let rows = read_rows(&bytes).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].date.to_string(), "2026-10-01");
        assert_eq!(rows[0].item, "");
        assert_eq!(rows[0].actual_in, Some(485));
        assert_eq!(rows[0].actual_out, Some(1021));
        assert_eq!(rows[1].item, "오전반차");
        assert_eq!(rows[1].actual_in, None);
        assert_eq!(rows[2].status, "휴일");
    }
}

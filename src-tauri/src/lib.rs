pub mod attendance;

use attendance::{
    calc::analyze,
    export::write_report,
    model::{Analysis, ItemRule, Summary, TodayPlan},
    parse::read_rows,
    rules::{load_rules, norm, save_rules},
    time::parse_hm,
    today::plan_today as calculate_today_plan,
};
use chrono::Local;
use std::{collections::BTreeMap, fs, path::PathBuf};
use tauri::{AppHandle, Manager};

fn rules_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join("item-rules.json"))
        .map_err(|error| format!("설정 폴더를 찾을 수 없습니다: {error}"))
}

fn analyze_path(app: &AppHandle, path: &str) -> Result<Analysis, String> {
    let bytes = fs::read(path).map_err(|error| format!("엑셀 파일을 읽을 수 없습니다: {error}"))?;
    let rows = read_rows(&bytes)?;
    let custom_rules = load_rules(&rules_path(app)?);
    Ok(analyze(&rows, &custom_rules, Local::now().date_naive()))
}

#[tauri::command]
fn analyze_file(app: AppHandle, path: String) -> Result<Analysis, String> {
    analyze_path(&app, &path)
}

#[tauri::command]
fn set_item_rules(app: AppHandle, rules: BTreeMap<String, ItemRule>) -> Result<(), String> {
    let path = rules_path(&app)?;
    let mut stored = load_rules(&path);
    for (item, rule) in rules {
        stored.insert(norm(&item), rule);
    }
    save_rules(&path, &stored)
}

#[tauri::command]
fn list_item_rules(app: AppHandle) -> Result<BTreeMap<String, ItemRule>, String> {
    Ok(load_rules(&rules_path(&app)?))
}

#[tauri::command]
fn delete_item_rule(app: AppHandle, item: String) -> Result<(), String> {
    let path = rules_path(&app)?;
    let mut stored = load_rules(&path);
    stored.remove(&norm(&item));
    save_rules(&path, &stored)
}

#[tauri::command]
fn export_report(app: AppHandle, path: String, out_path: String) -> Result<(), String> {
    let analysis = analyze_path(&app, &path)?;
    let Analysis::Ready { report } = analysis else {
        return Err("미지정 근태항목이 있어 저장할 수 없습니다".to_owned());
    };
    write_report(&report, PathBuf::from(out_path).as_path())
}

#[tauri::command]
fn plan_today(summary: Summary, arrival: String, bonus: bool) -> Result<TodayPlan, String> {
    let arrival = parse_hm(&arrival)
        .ok_or_else(|| "출근 시각 형식이 올바르지 않습니다 (HH:MM)".to_owned())?;
    Ok(calculate_today_plan(&summary, arrival, bonus))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            analyze_file,
            set_item_rules,
            list_item_rules,
            delete_item_rule,
            export_report,
            plan_today
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

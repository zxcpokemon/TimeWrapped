#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod native;
mod store;
mod tracker;
mod visibility;
mod inventory;
mod media;

use base64::Engine;
use store::{App, Dashboard, Settings, Store, Wrapped};

fn open_store() -> Result<Store, String> { Store::open(&Store::default_path()) }

#[tauri::command]
async fn get_dashboard(from: String, to: String) -> Result<Dashboard, String> {
    agent::flush()?;
    open_store()?.dashboard(&from, &to)
}
#[tauri::command]
async fn get_wrapped(week_start: String) -> Result<Wrapped, String> {
    agent::flush()?;
    open_store()?.wrapped(&week_start)
}
#[tauri::command]
async fn list_weeks() -> Result<Vec<String>, String> { agent::flush()?; open_store()?.list_weeks() }
#[tauri::command]
async fn get_settings() -> Result<Settings, String> { open_store()?.settings() }
#[tauri::command]
async fn save_settings(settings: Settings) -> Result<(), String> {
    let store = open_store()?;
    let previous=store.settings()?;
    store.save_settings(&settings)?;
    if previous.autostart!=settings.autostart {
        if let Err(error)=native::set_autostart(settings.autostart) {
            store.save_settings(&previous)?;
            return Err(error);
        }
    }
    agent::notify_settings_changed()?;
    Ok(())
}
#[tauri::command]
async fn get_apps() -> Result<Vec<App>, String> { agent::flush()?; open_store()?.apps() }
#[tauri::command]
async fn update_app(id: String, category: String, excluded: bool) -> Result<(), String> {
    agent::flush()?;
    open_store()?.update_app(&id, &category, excluded)?;
    agent::notify_settings_changed()?;
    Ok(())
}
#[tauri::command]
async fn clear_data() -> Result<(), String> {
    // Pause and flush across the process boundary before deleting. No buffered data can reappear.
    let mut store = open_store()?;
    let mut settings = store.settings()?;
    settings.tracking_enabled = false;
    store.save_settings(&settings)?;
    agent::notify_settings_changed()?;
    agent::flush()?;
    store.clear_data()?;
    let backup=Store::default_path().with_extension("pre-v0.2.sqlite3");
    match std::fs::remove_file(backup){Ok(())=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(e.to_string())}
    native::clear_icon_cache()?;
    for name in ["last-notified-week", "agent-error.log"] {
        let path=Store::default_path().with_file_name(name);
        match std::fs::remove_file(path) { Ok(())=>{}, Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{}, Err(e)=>return Err(e.to_string()) }
    }
    agent::notify_settings_changed()?;
    Ok(())
}
#[tauri::command]
async fn export_data(format: String) -> Result<Option<String>, String> {
    agent::flush()?;
    let store = open_store()?;
    let (name, filter, content) = match format.as_str() {
        "json" => ("TimeWrapped.json", "JSON files\0*.json\0\0", store.export_json()?),
        "csv" => ("TimeWrapped.csv", "CSV files\0*.csv\0\0", store.export_csv()?),
        _ => return Err("Неизвестный формат экспорта".into()),
    };
    native::save_file(name, filter, content.as_bytes())
}
#[tauri::command]
async fn save_card(png_base64: String) -> Result<Option<String>, String> {
    if png_base64.len() > 24_000_000 { return Err("Изображение слишком большое".into()); }
    let content = base64::engine::general_purpose::STANDARD.decode(png_base64.split(',').next_back().unwrap_or("")).map_err(|e| e.to_string())?;
    if !content.starts_with(b"\x89PNG\r\n\x1a\n") { return Err("Ожидалось изображение PNG".into()); }
    native::save_file("TimeWrapped-week.png", "PNG images\0*.png\0\0", &content)
}
#[tauri::command]
async fn get_diagnostics() -> Result<serde_json::Value, String> {
    agent::flush()?;
    let path = Store::default_path().with_file_name("agent-diagnostics.json");
    match std::fs::read_to_string(path) {
        Ok(value) => serde_json::from_str(&value).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({"status":"Сборщик запускается; измерения появятся после первой записи"})),
        Err(e) => Err(e.to_string()),
    }
}
#[tauri::command]
async fn get_app_icon(id: String) -> Result<Option<String>, String> { native::app_icon(&id) }

fn app_main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|s| s == "--quit") { agent::shutdown()?; return Ok(()); }
    if args.iter().any(|s| s == "--background") { return agent::run(Store::default_path()); }
    // Headless CLI utilities also allow reproducible isolation/performance checks.
    if args.iter().any(|s| s == "--diagnostics") {
        agent::flush()?;
        println!("{}", std::fs::read_to_string(Store::default_path().with_file_name("agent-diagnostics.json")).map_err(|e| e.to_string())?);
        return Ok(());
    }
    if args.iter().any(|s| s == "--export-json") {
        agent::flush()?;
        println!("{}", open_store()?.export_json()?);
        return Ok(());
    }
    agent::ensure_running()?;
    if open_store()?.settings()?.autostart{native::set_autostart(true)?;}
    let Some(_gui_guard) = native::gui_single_instance()? else { return Ok(()); };
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_dashboard, get_wrapped, list_weeks, get_settings, save_settings, get_apps, update_app, clear_data, export_data, save_card, get_diagnostics, get_app_icon])
        .run(tauri::generate_context!())
        .map_err(|e| e.to_string())
}

fn main() {
    if let Err(error) = app_main() {
        eprintln!("TimeWrapped: {error}");
        if std::env::args().any(|a|a.starts_with("--")) {
            let path=Store::default_path();
            if let Some(parent)=path.parent(){let _=std::fs::create_dir_all(parent);}
            let _=std::fs::write(path.with_file_name("agent-error.log"),&error);
        } else {native::show_error(&error);}
        std::process::exit(1);
    }
}

mod chats;
mod db;
mod ingestion;
mod library;
mod profile;
mod thumbnails;

use std::sync::Mutex;

use chats::{list_chat_messages, list_chat_threads};
use db::DbState;
use ingestion::run_ingestion;
use library::{list_memory_assets, list_memory_tags, set_asset_favorite, set_asset_tags};
use profile::get_profile_snapshot;
use tauri::Manager;
use thumbnails::generate_thumbnails;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn count_assets(state: tauri::State<DbState>) -> Result<i64, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    conn.query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn db_table_names(state: tauri::State<DbState>) -> Result<Vec<String>, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .map_err(|err| err.to_string())?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    Ok(names)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let conn = db::init(&app.handle())?;
            app.manage(DbState(Mutex::new(conn)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            count_assets,
            db_table_names,
            run_ingestion,
            generate_thumbnails,
            list_memory_assets,
            list_memory_tags,
            set_asset_favorite,
            set_asset_tags,
            list_chat_threads,
            list_chat_messages,
            get_profile_snapshot
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

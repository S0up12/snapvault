mod schema;

use std::sync::Mutex;

use rusqlite::Connection;
use tauri::{AppHandle, Manager};

pub struct DbState(pub Mutex<Connection>);

/// Opens (creating if needed) the app's SQLite database in the OS-specific
/// app data directory and applies the schema. Safe to call on every launch:
/// all DDL is `IF NOT EXISTS`.
pub fn init(app_handle: &AppHandle) -> Result<Connection, String> {
    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    std::fs::create_dir_all(&app_dir)
        .map_err(|err| format!("failed to create app data dir {}: {err}", app_dir.display()))?;

    let db_path = app_dir.join("metadata.db");
    let conn = Connection::open(&db_path)
        .map_err(|err| format!("failed to open database at {}: {err}", db_path.display()))?;

    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
        .map_err(|err| format!("failed to configure database connection: {err}"))?;
    conn.execute_batch(schema::SCHEMA_SQL)
        .map_err(|err| format!("failed to apply database schema: {err}"))?;

    Ok(conn)
}

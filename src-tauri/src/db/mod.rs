pub mod schema;

use std::sync::Mutex;

use rusqlite::Connection;
use tauri::{AppHandle, Manager};

pub struct DbState(pub Mutex<Connection>);

/// Opens (creating if needed) the app's SQLite database in the OS-specific
/// app data directory and applies the schema. Safe to call on every launch:
/// all DDL is `IF NOT EXISTS` - except `CREATE TABLE IF NOT EXISTS` is a
/// no-op against a table that already exists with an older column set, so
/// columns added after a user's db file was first created also need an
/// explicit `ALTER TABLE ... ADD COLUMN` migration below.
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

    // ponytail: ad-hoc per-column migrations, fine while there's only one.
    // If more schema changes land after users have existing databases,
    // replace this with a versioned migration list.
    add_column_if_missing(&conn, "assets", "latitude", "REAL")?;
    add_column_if_missing(&conn, "assets", "longitude", "REAL")?;
    add_column_if_missing(&conn, "assets", "playback_path", "TEXT")?;

    Ok(conn)
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    column_ddl: &str,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1"))
        .map_err(|err| format!("failed to inspect {table} schema: {err}"))?;
    let exists = stmt
        .exists([column])
        .map_err(|err| format!("failed to inspect {table} schema: {err}"))?;
    if !exists {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {column_ddl}"), [])
            .map_err(|err| format!("failed to add column {table}.{column}: {err}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_missing_column_and_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        // Simulate a persistent db created before `latitude` existed.
        conn.execute_batch(
            "CREATE TABLE assets (id TEXT PRIMARY KEY, original_path TEXT NOT NULL UNIQUE);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets (id, original_path) VALUES ('a1', '/tmp/a')",
            [],
        )
        .unwrap();

        add_column_if_missing(&conn, "assets", "latitude", "REAL").unwrap();
        // Running it again against a table that already has the column
        // must not error (this runs on every app launch).
        add_column_if_missing(&conn, "assets", "latitude", "REAL").unwrap();

        conn.execute(
            "UPDATE assets SET latitude = 51.5 WHERE id = 'a1'",
            [],
        )
        .unwrap();
        let latitude: f64 = conn
            .query_row("SELECT latitude FROM assets WHERE id = 'a1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(latitude, 51.5);
    }
}

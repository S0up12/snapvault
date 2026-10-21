pub mod schema;

use std::sync::Mutex;

use rusqlite::Connection;
use tauri::{AppHandle, Manager};

pub(crate) const PRAGMAS: &str = "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;";

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

    conn.execute_batch(PRAGMAS)
        .map_err(|err| format!("failed to configure database connection: {err}"))?;
    conn.execute_batch(schema::SCHEMA_SQL)
        .map_err(|err| format!("failed to apply database schema: {err}"))?;

    // ponytail: ad-hoc per-column migrations, fine while there's only one.
    // If more schema changes land after users have existing databases,
    // replace this with a versioned migration list.
    add_column_if_missing(&conn, "assets", "latitude", "REAL")?;
    add_column_if_missing(&conn, "assets", "longitude", "REAL")?;
    add_column_if_missing(&conn, "assets", "playback_path", "TEXT")?;
    add_column_if_missing(&conn, "assets", "thumb_failed", "INTEGER")?;
    add_column_if_missing(&conn, "chat_messages", "is_sender", "INTEGER")?;
    backfill_is_sender(&conn)?;

    Ok(conn)
}

/// One-off for rows ingested before `is_sender` existed; a no-op (index-less
/// scan finds nothing) afterwards. Rows with missing/invalid payload stay NULL
/// and are resolved at read time.
fn backfill_is_sender(conn: &Connection) -> Result<(), String> {
    conn.execute(
        "UPDATE chat_messages
         SET is_sender = CASE json_extract(raw_payload, '$.IsSender') WHEN 1 THEN 1 WHEN 0 THEN 0 END
         WHERE is_sender IS NULL AND raw_payload IS NOT NULL AND json_valid(raw_payload)",
        [],
    )
    .map_err(|err| format!("failed to backfill chat_messages.is_sender: {err}"))?;
    Ok(())
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

    #[test]
    fn backfills_is_sender_and_skips_invalid_payloads() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(schema::SCHEMA_SQL).unwrap();
        conn.execute("INSERT INTO chat_threads (id, external_id) VALUES ('t', 'e')", []).unwrap();
        for (id, payload) in [("a", "{\"IsSender\":true}"), ("b", "{\"IsSender\":false}"), ("c", "not json"), ("d", "{}")] {
            conn.execute(
                "INSERT INTO chat_messages (id, thread_id, sender, sent_at, dedupe_key, raw_payload) VALUES (?1, 't', 's', 'x', ?1, ?2)",
                [id, payload],
            )
            .unwrap();
        }
        backfill_is_sender(&conn).unwrap();
        let got: Vec<Option<i64>> = conn
            .prepare("SELECT is_sender FROM chat_messages ORDER BY id").unwrap()
            .query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(got, vec![Some(1), Some(0), None, None]);
    }
}

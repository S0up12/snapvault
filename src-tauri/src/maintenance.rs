use std::path::Path;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::db::DbState;

#[derive(Clone, Serialize)]
pub struct LibraryStats {
    pub total_assets: i64,
    pub images: i64,
    pub videos: i64,
    pub audio: i64,
    pub thumbnails_missing: i64,
    pub playback_pending: i64,
    pub memory_items: i64,
    pub chat_threads: i64,
    pub chat_messages: i64,
    pub chat_media_linked: i64,
    pub profile_found: bool,
    pub db_size_bytes: i64,
}

/// Snapshot of library health for the Settings view - the same counts this
/// project's own testing has repeatedly needed to check by hand (asset
/// totals, how many videos still need a playback conversion, whether chat
/// media actually got linked).
#[tauri::command]
pub fn get_library_stats(app: AppHandle, state: tauri::State<DbState>) -> Result<LibraryStats, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;

    let count = |sql: &str| -> Result<i64, String> { conn.query_row(sql, [], |row| row.get(0)).map_err(|err| err.to_string()) };

    let db_path = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?
        .join("metadata.db");
    let db_size_bytes = std::fs::metadata(&db_path).map(|m| m.len() as i64).unwrap_or(0);

    Ok(LibraryStats {
        total_assets: count("SELECT COUNT(*) FROM assets")?,
        images: count("SELECT COUNT(*) FROM assets WHERE media_type = 'image'")?,
        videos: count("SELECT COUNT(*) FROM assets WHERE media_type = 'video'")?,
        audio: count("SELECT COUNT(*) FROM assets WHERE media_type = 'audio'")?,
        thumbnails_missing: count(
            "SELECT COUNT(*) FROM assets WHERE media_type IN ('image', 'video') AND thumbnail_path IS NULL",
        )?,
        playback_pending: count("SELECT COUNT(*) FROM assets WHERE media_type = 'video' AND playback_path IS NULL")?,
        memory_items: count("SELECT COUNT(*) FROM memory_items")?,
        chat_threads: count("SELECT COUNT(*) FROM chat_threads")?,
        chat_messages: count("SELECT COUNT(*) FROM chat_messages")?,
        chat_media_linked: count("SELECT COUNT(*) FROM chat_message_assets")?,
        profile_found: count("SELECT COUNT(*) FROM profile_snapshots")? > 0,
        db_size_bytes,
    })
}

#[derive(Clone, Serialize)]
pub struct VerifySummary {
    pub checked: usize,
    pub missing_original: usize,
    pub missing_thumbnail: usize,
    pub missing_playback: usize,
}

/// Walks every asset's `original_path`/`thumbnail_path`/`playback_path` and
/// checks the file still exists on disk - catches a moved/deleted app-data
/// folder or a partially-failed media processing run without having to
/// spot-check paths by hand.
#[tauri::command]
pub async fn verify_library(state: tauri::State<'_, DbState>) -> Result<VerifySummary, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    verify_library_blocking(&conn)
}

fn verify_library_blocking(conn: &Connection) -> Result<VerifySummary, String> {
    let mut stmt = conn
        .prepare("SELECT original_path, thumbnail_path, playback_path FROM assets")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;

    let mut summary = VerifySummary {
        checked: rows.len(),
        missing_original: 0,
        missing_thumbnail: 0,
        missing_playback: 0,
    };
    for (original_path, thumbnail_path, playback_path) in &rows {
        if !Path::new(original_path).is_file() {
            summary.missing_original += 1;
        }
        if thumbnail_path.as_deref().is_some_and(|path| !Path::new(path).is_file()) {
            summary.missing_thumbnail += 1;
        }
        if playback_path.as_deref().is_some_and(|path| !Path::new(path).is_file()) {
            summary.missing_playback += 1;
        }
    }
    Ok(summary)
}

/// Wipes every table and deletes the `imports`/`thumbnails`/`playback`
/// directories, for starting a clean re-import during testing. Deletes rows
/// via the existing connection rather than the `.db` file itself, since
/// SQLite holds that file open for the app's whole lifetime.
#[tauri::command]
pub async fn reset_library(app: AppHandle, state: tauri::State<'_, DbState>) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;

    {
        let conn = state.0.lock().map_err(|err| err.to_string())?;
        reset_library_blocking(&conn)?;
    }

    for dir_name in ["imports", "thumbnails", "playback"] {
        let dir = app_data_dir.join(dir_name);
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir).map_err(|err| format!("failed to remove {}: {err}", dir.display()))?;
        }
    }

    Ok(())
}

fn reset_library_blocking(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "PRAGMA foreign_keys = OFF;
         DELETE FROM chat_message_assets;
         DELETE FROM chat_messages;
         DELETE FROM chat_threads;
         DELETE FROM story_items;
         DELETE FROM story_collections;
         DELETE FROM memory_items;
         DELETE FROM memory_collections;
         DELETE FROM assets;
         DELETE FROM ingestion_jobs;
         DELETE FROM profile_snapshots;
         PRAGMA foreign_keys = ON;
         VACUUM;",
    )
    .map_err(|err| format!("failed to reset database: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;

    fn seeded_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, thumbnail_path, playback_path)
             VALUES ('present', 'image', '/exists.jpg', '/exists.webp', NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, thumbnail_path, playback_path)
             VALUES ('missing', 'video', '/gone.mp4', '/gone.webp', '/gone-playback.mp4')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn verify_library_counts_missing_files_per_column() {
        let conn = seeded_conn();
        let summary = verify_library_blocking(&conn).unwrap();
        assert_eq!(summary.checked, 2);
        assert_eq!(summary.missing_original, 2);
        assert_eq!(summary.missing_thumbnail, 2);
        assert_eq!(summary.missing_playback, 1);
    }

    #[test]
    fn verify_library_ignores_null_thumbnail_and_playback() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path) VALUES ('a', 'image', '/gone.jpg')",
            [],
        )
        .unwrap();

        let summary = verify_library_blocking(&conn).unwrap();
        assert_eq!(summary.missing_original, 1);
        assert_eq!(summary.missing_thumbnail, 0);
        assert_eq!(summary.missing_playback, 0);
    }

    #[test]
    fn reset_library_clears_every_table() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();

        conn.execute("INSERT INTO memory_collections (id, title) VALUES ('c1', 'Test')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path) VALUES ('a1', 'image', '/a.jpg')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO memory_items (id, collection_id, asset_id) VALUES ('m1', 'c1', 'a1')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO chat_threads (id, external_id) VALUES ('t1', 'ext')", [])
            .unwrap();

        reset_library_blocking(&conn).unwrap();

        for table in ["assets", "memory_items", "memory_collections", "chat_threads"] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 0, "table {table} should be empty after reset");
        }
    }
}

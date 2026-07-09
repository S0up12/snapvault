use serde::Serialize;
use serde_json::Value;

use crate::db::DbState;

#[derive(Clone, Serialize)]
pub struct ProfileSnapshot {
    pub generated_at: Option<String>,
    pub snapshot: Value,
    pub memory_count: i64,
}

/// Returns the consolidated profile snapshot built during ingestion
/// (`ingestion::profile::build_profile_snapshot_blocking`), plus a live
/// memory count from the `assets` table - the one "profile stat" that isn't
/// sourced from the Snapchat export JSON at all, so it's always current
/// rather than frozen at last-import time.
#[tauri::command]
pub fn get_profile_snapshot(state: tauri::State<DbState>) -> Result<Option<ProfileSnapshot>, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;

    let row: Option<(Option<String>, String)> = conn
        .query_row(
            "SELECT generated_at, snapshot FROM profile_snapshots WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    let Some((generated_at, snapshot_text)) = row else {
        return Ok(None);
    };
    let snapshot: Value = serde_json::from_str(&snapshot_text).map_err(|err| err.to_string())?;

    let memory_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE source_type = 'memory'",
            [],
            |row| row.get(0),
        )
        .map_err(|err| err.to_string())?;

    Ok(Some(ProfileSnapshot {
        generated_at,
        snapshot,
        memory_count,
    }))
}

#[cfg(test)]
mod tests {
    use crate::db::schema::SCHEMA_SQL;
    use rusqlite::Connection;

    #[test]
    fn returns_none_when_no_snapshot_exists() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();

        let row: Option<(Option<String>, String)> = conn
            .query_row("SELECT generated_at, snapshot FROM profile_snapshots WHERE id = 1", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .ok();
        assert!(row.is_none());
    }

    #[test]
    fn memory_count_only_counts_memory_source_type() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, source_type) VALUES ('a', 'image', '/a.jpg', 'memory')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, source_type) VALUES ('b', 'image', '/b.jpg', 'chat')",
            [],
        )
        .unwrap();

        let memory_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM assets WHERE source_type = 'memory'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(memory_count, 1);
    }
}

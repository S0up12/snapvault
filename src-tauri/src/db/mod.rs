pub mod schema;

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::{AppHandle, Manager};

pub(crate) const PRAGMAS: &str = "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;";

pub struct DbState(pub Mutex<Connection>);

/// Second arg = current media root (needed by path-rewriting migrations).
type Migration = fn(&Connection, &Path) -> rusqlite::Result<()>;

/// Ordered schema migrations. `PRAGMA user_version` = number of entries already
/// applied, so entry `i` upgrades version `i` -> `i + 1`.
///
/// HOW TO ADD A MIGRATION: append a `fn(&Connection) -> rusqlite::Result<()>`
/// to the END of this list (never reorder, edit, or delete shipped entries).
/// It runs inside a transaction together with the version bump, so a failure
/// rolls back and the app refuses to start rather than run on a half-migrated
/// db. Do NOT edit `schema::SCHEMA_SQL` for changes: it is frozen as part of the
/// v1 baseline, and fresh databases replay every later migration on top of it.
/// Plain DDL (ALTER TABLE, CREATE INDEX, table rebuild for CHECK/type changes)
/// is fine; avoid `PRAGMA`s that cannot run inside a transaction.
pub(crate) const MIGRATIONS: &[Migration] = &[migrate_v1_baseline, migrate_v2_relative_paths];

/// v1 baseline = schema.rs + every column that used to be added ad hoc + the
/// is_sender backfill. Idempotent, so it is also correct for legacy databases
/// (tables exist, user_version = 0): existing rows are kept, missing columns added.
fn migrate_v1_baseline(conn: &Connection, _root: &Path) -> rusqlite::Result<()> {
    conn.execute_batch(schema::SCHEMA_SQL)?;
    add_column_if_missing(conn, "assets", "latitude", "REAL")?;
    add_column_if_missing(conn, "assets", "longitude", "REAL")?;
    add_column_if_missing(conn, "assets", "playback_path", "TEXT")?;
    add_column_if_missing(conn, "assets", "thumb_failed", "INTEGER")?;
    add_column_if_missing(conn, "chat_messages", "is_sender", "INTEGER")?;
    backfill_is_sender(conn)
}

/// v2: asset path columns become media-root-relative (forward slashes). Rows
/// not under the current root (other drive, already relative) are left as-is;
/// `storage::to_absolute` reads both forms, so skipping is harmless.
fn migrate_v2_relative_paths(conn: &Connection, root: &Path) -> rusqlite::Result<()> {
    type Row = (String, String, Option<String>, Option<String>, Option<String>);
    let rows: Vec<Row> = conn
        .prepare("SELECT id, original_path, thumbnail_path, playback_path, overlay_path FROM assets")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let rel = |p: &str| crate::storage::to_relative(root, p);
    for (id, orig, thumb, play, overlay) in rows {
        let new = (rel(&orig), thumb.as_deref().map(rel), play.as_deref().map(rel), overlay.as_deref().map(rel));
        if new != (orig, thumb, play, overlay) {
            conn.execute(
                "UPDATE assets SET original_path = ?1, thumbnail_path = ?2, playback_path = ?3, overlay_path = ?4 WHERE id = ?5",
                rusqlite::params![new.0, new.1, new.2, new.3, id],
            )?;
        }
    }
    Ok(())
}

/// Applies pending migrations, each in its own transaction with its version bump.
pub(crate) fn migrate(conn: &Connection, migrations: &[Migration], root: &Path) -> Result<(), String> {
    let current: usize = conn
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(|err| format!("failed to read schema version: {err}"))?
        .try_into()
        .unwrap_or(0);
    if current > migrations.len() {
        return Err(format!(
            "database schema version {current} is newer than this app supports ({})",
            migrations.len()
        ));
    }
    for (i, migration) in migrations.iter().enumerate().skip(current) {
        let version = i + 1;
        let run = || -> rusqlite::Result<()> {
            let tx = conn.unchecked_transaction()?; // rolls back on drop unless committed
            migration(&tx, root)?;
            tx.execute_batch(&format!("PRAGMA user_version = {version}"))?;
            tx.commit()
        };
        run().map_err(|err| format!("database migration to version {version} failed: {err}"))?;
        crate::log::log_info!("database migrated to schema version {version}");
    }
    Ok(())
}

/// Opens (creating if needed) the app's SQLite database in the OS-specific
/// app data directory and brings it to the latest schema via `migrate`.
/// Safe to call on every launch.
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
    let media_root = crate::storage::resolve_media_root(app_handle)?;
    migrate(&conn, MIGRATIONS, &media_root)?;

    Ok(conn)
}

/// One-off for rows ingested before `is_sender` existed; a no-op (index-less
/// scan finds nothing) afterwards. Rows with missing/invalid payload stay NULL
/// and are resolved at read time.
fn backfill_is_sender(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE chat_messages
         SET is_sender = CASE json_extract(raw_payload, '$.IsSender') WHEN 1 THEN 1 WHEN 0 THEN 0 END
         WHERE is_sender IS NULL AND raw_payload IS NOT NULL AND json_valid(raw_payload)",
        [],
    )?;
    Ok(())
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    column_ddl: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1"))?;
    if !stmt.exists([column])? {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {column_ddl}"), [])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
    }

    fn tables(conn: &Connection) -> Vec<String> {
        conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    #[test]
    fn fresh_db_gets_full_schema_and_version() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn, MIGRATIONS, Path::new("/none")).unwrap();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
        assert_eq!(tables(&conn).len(), 10);
        conn.execute("UPDATE assets SET latitude = 1 WHERE 0", []).unwrap();
    }

    #[test]
    fn legacy_db_is_upgraded_without_data_loss() {
        let conn = Connection::open_in_memory().unwrap();
        // Old shape: no latitude/longitude/playback_path/thumb_failed, no is_sender; user_version 0.
        conn.execute_batch(
            r#"CREATE TABLE assets (id TEXT PRIMARY KEY, external_id TEXT,
                source_type TEXT NOT NULL DEFAULT 'unknown', media_type TEXT NOT NULL,
                original_path TEXT NOT NULL UNIQUE, thumbnail_path TEXT, overlay_path TEXT,
                checksum_sha256 TEXT, file_size_bytes INTEGER, taken_at TEXT,
                is_favorite INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '[]',
                raw_metadata TEXT,
                created_at TEXT NOT NULL DEFAULT 'x', updated_at TEXT NOT NULL DEFAULT 'x');
             CREATE TABLE chat_threads (id TEXT PRIMARY KEY, external_id TEXT NOT NULL UNIQUE, title TEXT,
                is_group INTEGER NOT NULL DEFAULT 0, raw_metadata TEXT,
                created_at TEXT NOT NULL DEFAULT 'x', updated_at TEXT NOT NULL DEFAULT 'x');
             CREATE TABLE chat_messages (id TEXT PRIMARY KEY, thread_id TEXT NOT NULL, sender TEXT NOT NULL,
                body TEXT, sent_at TEXT NOT NULL, message_type TEXT NOT NULL DEFAULT 'TEXT',
                source TEXT NOT NULL DEFAULT 'chat_history', dedupe_key TEXT NOT NULL UNIQUE,
                raw_media_ids TEXT, raw_payload TEXT,
                created_at TEXT NOT NULL DEFAULT 'x', updated_at TEXT NOT NULL DEFAULT 'x');
             INSERT INTO assets (id, media_type, original_path) VALUES ('a1', 'image', 'p');
             INSERT INTO chat_threads (id, external_id) VALUES ('t', 'e');
             INSERT INTO chat_messages (id, thread_id, sender, sent_at, dedupe_key, raw_payload)
                VALUES ('m1', 't', 's', 'x', 'k', '{"IsSender":true}');"#,
        )
        .unwrap();
        assert_eq!(version(&conn), 0);
        migrate(&conn, MIGRATIONS, Path::new("/none")).unwrap();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
        assert_eq!(tables(&conn).len(), 10);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM assets WHERE id='a1' AND thumb_failed IS NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let s: Option<i64> = conn
            .query_row("SELECT is_sender FROM chat_messages WHERE id='m1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(s, Some(1));
    }

    #[test]
    fn v2_rewrites_absolute_paths_under_root_and_round_trips() {
        use crate::storage::{to_absolute, to_relative};
        let root = std::env::temp_dir().join("sv-root-test");
        let s = |p: PathBuf| p.display().to_string();
        let orig = s(root.join("imports").join("j").join("part-001").join("a.jpg"));
        let thumb = s(root.join("thumbnails").join("a1.webp"));
        let outside = s(std::env::temp_dir().join("sv-other-drive").join("b.jpg"));

        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn, &MIGRATIONS[..1], &root).unwrap(); // v1 only
        for (id, o, t) in [("a1", &orig, Some(&thumb)), ("b1", &outside, None)] {
            conn.execute(
                "INSERT INTO assets (id, media_type, original_path, thumbnail_path, playback_path) VALUES (?1, 'image', ?2, ?3, ?2)",
                rusqlite::params![id, o, t],
            )
            .unwrap();
        }
        migrate(&conn, MIGRATIONS, &root).unwrap();
        migrate(&conn, MIGRATIONS, &root).unwrap(); // idempotent

        let get = |id: &str| -> (String, Option<String>, Option<String>) {
            conn.query_row(
                "SELECT original_path, thumbnail_path, playback_path FROM assets WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };
        let (o, t, p) = get("a1");
        assert_eq!(o, "imports/j/part-001/a.jpg");
        assert_eq!(t.as_deref(), Some("thumbnails/a1.webp"));
        assert_eq!(p.as_deref(), Some("imports/j/part-001/a.jpg"));
        assert_eq!(to_absolute(&root, &o), PathBuf::from(&orig));
        assert_eq!(to_absolute(&root, &t.unwrap()), PathBuf::from(&thumb));
        let (o, t, _) = get("b1");
        assert_eq!((o.as_str(), t), (outside.as_str(), None)); // untouched
        assert_eq!(to_absolute(&root, &o), PathBuf::from(&outside));
        assert_eq!(to_relative(&root, &outside), outside);
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn reopen_is_noop() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn, MIGRATIONS, Path::new("/none")).unwrap();
        conn.execute("INSERT INTO chat_threads (id, external_id) VALUES ('t', 'e')", []).unwrap();
        migrate(&conn, MIGRATIONS, Path::new("/none")).unwrap();
        assert_eq!(version(&conn), MIGRATIONS.len() as i64);
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM chat_threads", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn failed_migration_rolls_back_and_keeps_version() {
        fn ok(c: &Connection, _: &Path) -> rusqlite::Result<()> {
            c.execute_batch("CREATE TABLE t1 (x)")
        }
        fn bad(c: &Connection, _: &Path) -> rusqlite::Result<()> {
            c.execute_batch("CREATE TABLE t2 (x)")?;
            c.execute_batch("THIS IS NOT SQL")
        }
        let conn = Connection::open_in_memory().unwrap();
        let err = migrate(&conn, &[ok, bad], Path::new("/none")).unwrap_err();
        assert!(err.contains("version 2"));
        assert_eq!(version(&conn), 1);
        assert_eq!(tables(&conn), vec!["t1".to_string()]);
    }

    #[test]
    fn newer_db_is_rejected() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version = 99").unwrap();
        assert!(migrate(&conn, MIGRATIONS, Path::new("/none")).is_err());
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

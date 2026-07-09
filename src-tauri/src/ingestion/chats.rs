use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::memories::{media_type_for_extension, parse_snap_date};

#[derive(Clone, Serialize)]
pub struct ParseChatsSummary {
    pub threads_found: usize,
    pub threads_inserted: usize,
    pub messages_found: usize,
    pub messages_inserted: usize,
    pub media_assets_linked: usize,
}

/// Reads `chat_history.json` (conversation id -> message list) and populates
/// `chat_threads`/`chat_messages`, linking each message to its media file(s)
/// (if any) under `chat_media/` via `Media IDs` - see `load_chat_media_files`
/// and `parse_media_ids` for the matching scheme, ported from the old app's
/// `IngestionService.find_assets_for_message`. A conversation is a group if
/// any of its messages carries a non-empty `Conversation Title` distinct
/// from its own id - Snapchat only ever sets that field for group chats.
/// 1:1 threads with no title get their counterpart's `friends.json` display
/// name instead of the raw username, when available.
///
/// `snap_history.json` (ephemeral snaps) is intentionally not parsed here -
/// the old app matched those to `chat_media/` by nearest timestamp within a
/// shared date bucket rather than an explicit id, and nothing in this app
/// reads that file yet.
pub(super) fn parse_chats_blocking(
    conn: &Connection,
    job_dir: &std::path::Path,
    emit: &dyn Fn(usize, usize, String),
) -> Result<ParseChatsSummary, String> {
    emit(0, 0, "Parsing chat history".to_string());

    let part_dirs = super::find_part_dirs(job_dir)?;
    let conversations = load_chat_history(&part_dirs)?;
    let friends_map = load_friends_display_names(&part_dirs)?;
    let chat_media_files = load_chat_media_files(&part_dirs)?;

    let total_messages: usize = conversations.values().map(|v| v.len()).sum();
    emit(
        0,
        total_messages,
        format!("Importing {} conversations", conversations.len()),
    );
    let emit_step = (total_messages / 100).max(1);

    let mut threads_inserted = 0usize;
    let mut messages_inserted = 0usize;
    let mut media_assets_linked = 0usize;
    let mut processed = 0usize;

    for (external_id, entries) in &conversations {
        let title = entries.iter().find_map(|entry| {
            let title = entry.get("Conversation Title").and_then(|v| v.as_str())?;
            (!title.is_empty() && title != external_id).then(|| title.to_string())
        });
        let is_group = title.is_some();
        let display_title = title.or_else(|| friends_map.get(external_id).cloned());

        let (thread_id, inserted) =
            upsert_chat_thread(conn, external_id, display_title.as_deref(), is_group)?;
        if inserted {
            threads_inserted += 1;
        }

        for entry in entries {
            let sender = entry
                .get("From")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let message_type = entry
                .get("Media Type")
                .and_then(|v| v.as_str())
                .unwrap_or("TEXT")
                .to_string();
            let body = entry.get("Content").and_then(|v| v.as_str()).map(str::to_string);
            let raw_media_ids = entry.get("Media IDs").and_then(|v| v.as_str()).map(str::to_string);
            let created_raw = entry.get("Created").and_then(|v| v.as_str());
            let Some((_, sent_at)) = created_raw.and_then(parse_snap_date) else {
                continue;
            };
            let microseconds = entry
                .get("Created(microseconds)")
                .map(|v| v.to_string())
                .unwrap_or_default();
            let dedupe_key = format!("{external_id}:{sender}:{sent_at}:{microseconds}");

            let inserted_count = conn
                .execute(
                    "INSERT OR IGNORE INTO chat_messages
                     (id, thread_id, sender, body, sent_at, message_type, source, dedupe_key, raw_media_ids, raw_payload)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'chat_history', ?7, ?8, ?9)",
                    rusqlite::params![
                        Uuid::new_v4().to_string(),
                        thread_id,
                        sender,
                        body,
                        sent_at,
                        message_type,
                        dedupe_key,
                        raw_media_ids,
                        entry.to_string(),
                    ],
                )
                .map_err(|err| format!("failed to insert chat_message: {err}"))?;
            messages_inserted += inserted_count;

            let media_ids = raw_media_ids.as_deref().map(parse_media_ids).unwrap_or_default();
            if !media_ids.is_empty() {
                // Re-ingestion hits the `INSERT OR IGNORE` above and the
                // generated uuid above is discarded - look the message back
                // up by its stable dedupe_key so linking still targets the
                // right row either way.
                let message_id: String = conn
                    .query_row(
                        "SELECT id FROM chat_messages WHERE dedupe_key = ?1",
                        [&dedupe_key],
                        |row| row.get(0),
                    )
                    .map_err(|err| format!("failed to look up chat_message {dedupe_key}: {err}"))?;

                for media_id in media_ids {
                    let Some(path) = chat_media_files.get(&media_id) else {
                        continue;
                    };
                    let asset_id = upsert_chat_asset(conn, path)?;
                    let linked = conn
                        .execute(
                            "INSERT OR IGNORE INTO chat_message_assets (message_id, asset_id) VALUES (?1, ?2)",
                            rusqlite::params![message_id, asset_id],
                        )
                        .map_err(|err| format!("failed to link chat_message_assets: {err}"))?;
                    media_assets_linked += linked;
                }
            }

            processed += 1;
            if processed % emit_step == 0 || processed == total_messages {
                emit(
                    processed,
                    total_messages,
                    format!("Importing chats {processed}/{total_messages}"),
                );
            }
        }
    }

    Ok(ParseChatsSummary {
        threads_found: conversations.len(),
        threads_inserted,
        messages_found: total_messages,
        messages_inserted,
        media_assets_linked,
    })
}

/// Splits a `Media IDs` field into individual ids - the old app's
/// `parse_media_ids` treats it as `|`/`,`-delimited to support messages with
/// more than one attachment, even though single-attachment messages (the
/// only kind seen so far) make this just a one-element vec.
fn parse_media_ids(raw: &str) -> Vec<String> {
    raw.split(['|', ','])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Indexes `chat_media/` files across every part by the Snapchat media id
/// embedded in their filename, so `Media IDs` entries can be resolved to an
/// actual file. Extracted filenames look like
/// `2017-04-30_b~EiQSFUJSSEpFMDd3QW40S1hZRWRvSWd0aBoAGgAyAXxIAlAEYAE.jpg`
/// (`{date}_{media_id}.{ext}`); only ported the `b~...`-id scheme the old
/// app's `extract_chat_media_id` recognizes for `chat_history.json` - the
/// `media~`/`overlay~`/`thumbnail~zip-<uuid>` filenames alongside them belong
/// to `snap_history.json`, which isn't parsed here (see doc comment above).
fn load_chat_media_files(part_dirs: &[PathBuf]) -> Result<HashMap<String, PathBuf>, String> {
    let mut map = HashMap::new();
    for part_dir in part_dirs {
        let media_dir = part_dir.join("chat_media");
        if !media_dir.is_dir() {
            continue;
        }
        let entries = fs::read_dir(&media_dir)
            .map_err(|err| format!("failed to read {}: {err}", media_dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|err| err.to_string())?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some((date_part, media_id)) = stem.split_once('_') else {
                continue;
            };
            if !is_date_prefix(date_part) || !media_id.starts_with("b~") {
                continue;
            }
            map.entry(media_id.to_string()).or_insert(path);
        }
    }
    Ok(map)
}

fn is_date_prefix(s: &str) -> bool {
    s.len() == 10
        && s.as_bytes()[4] == b'-'
        && s.as_bytes()[7] == b'-'
        && s.bytes().enumerate().all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit())
}

/// Upserts a `chat`-sourced asset for a matched `chat_media/` file, keyed by
/// `original_path` like `memories::upsert_asset` - re-ingestion resolves to
/// the same row instead of duplicating it.
fn upsert_chat_asset(conn: &Connection, path: &Path) -> Result<String, String> {
    let original_path = path.display().to_string();
    if let Ok(id) = conn.query_row(
        "SELECT id FROM assets WHERE original_path = ?1",
        [&original_path],
        |row| row.get::<_, String>(0),
    ) {
        return Ok(id);
    }

    let id = Uuid::new_v4().to_string();
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let media_type = media_type_for_extension(extension);
    let file_size_bytes = fs::metadata(path).map(|m| m.len()).ok();

    conn.execute(
        "INSERT INTO assets (id, source_type, media_type, original_path, file_size_bytes)
         VALUES (?1, 'chat', ?2, ?3, ?4)",
        rusqlite::params![id, media_type, original_path, file_size_bytes],
    )
    .map_err(|err| format!("failed to insert chat asset: {err}"))?;
    Ok(id)
}

fn load_chat_history(part_dirs: &[PathBuf]) -> Result<HashMap<String, Vec<Value>>, String> {
    let mut merged: HashMap<String, Vec<Value>> = HashMap::new();
    for part_dir in part_dirs {
        let path = part_dir.join("json").join("chat_history.json");
        if !path.is_file() {
            continue;
        }
        let contents = fs::read_to_string(&path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let payload: Value = serde_json::from_str(&contents)
            .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
        let Value::Object(map) = payload else {
            continue;
        };
        for (key, value) in map {
            let Value::Array(entries) = value else {
                continue;
            };
            merged.entry(key).or_default().extend(entries);
        }
    }
    Ok(merged)
}

fn load_friends_display_names(part_dirs: &[PathBuf]) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    for part_dir in part_dirs {
        let path = part_dir.join("json").join("friends.json");
        if !path.is_file() {
            continue;
        }
        let contents = fs::read_to_string(&path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let payload: Value = serde_json::from_str(&contents)
            .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
        let Some(friends) = payload.get("Friends").and_then(|v| v.as_array()) else {
            continue;
        };
        for friend in friends {
            let username = friend.get("Username").and_then(|v| v.as_str());
            let display_name = friend.get("Display Name").and_then(|v| v.as_str());
            if let (Some(username), Some(display_name)) = (username, display_name) {
                if !display_name.is_empty() {
                    map.insert(username.to_string(), display_name.to_string());
                }
            }
        }
    }
    Ok(map)
}

fn upsert_chat_thread(
    conn: &Connection,
    external_id: &str,
    title: Option<&str>,
    is_group: bool,
) -> Result<(String, bool), String> {
    if let Ok(id) = conn.query_row(
        "SELECT id FROM chat_threads WHERE external_id = ?1",
        [external_id],
        |row| row.get::<_, String>(0),
    ) {
        // Keep title/is_group fresh: a re-ingestion might resolve a friend's
        // display name or discover a group's title that wasn't known before.
        conn.execute(
            "UPDATE chat_threads SET title = ?1, is_group = ?2 WHERE id = ?3",
            rusqlite::params![title, is_group as i64, id],
        )
        .map_err(|err| err.to_string())?;
        return Ok((id, false));
    }

    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO chat_threads (id, external_id, title, is_group) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![id, external_id, title, is_group as i64],
    )
    .map_err(|err| format!("failed to insert chat_thread: {err}"))?;
    Ok((id, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("snapvault-chats-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn part_dir_with_chat_history(payload: &Value) -> PathBuf {
        let job_dir = tempdir();
        let part_dir = job_dir.join("part-000");
        let json_dir = part_dir.join("json");
        fs::create_dir_all(&json_dir).unwrap();
        fs::write(json_dir.join("chat_history.json"), serde_json::to_string(payload).unwrap()).unwrap();
        part_dir
    }

    #[test]
    fn parse_media_ids_splits_on_pipe_and_comma_and_drops_blanks() {
        assert_eq!(parse_media_ids(""), Vec::<String>::new());
        assert_eq!(parse_media_ids("b~abc"), vec!["b~abc".to_string()]);
        assert_eq!(
            parse_media_ids("b~abc|b~def,b~ghi"),
            vec!["b~abc".to_string(), "b~def".to_string(), "b~ghi".to_string()]
        );
    }

    #[test]
    fn load_chat_media_files_indexes_b_tilde_ids_and_skips_snap_history_scheme() {
        let job_dir = tempdir();
        let part_dir = job_dir.join("part-000");
        let media_dir = part_dir.join("chat_media");
        fs::create_dir_all(&media_dir).unwrap();
        fs::write(media_dir.join("2017-04-30_b~EiQSFabc.jpg"), b"fake-jpg").unwrap();
        // snap_history's media~/overlay~/thumbnail~zip-<uuid> scheme has no
        // matching "Media IDs" entry and must be ignored, not misindexed.
        fs::write(
            media_dir.join("2017-09-08_media~Snapchat-648253424.zip.nomedia.mp4"),
            b"fake-mp4",
        )
        .unwrap();

        let files = load_chat_media_files(&[part_dir]).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files.contains_key("b~EiQSFabc"));
    }

    #[test]
    fn media_message_gets_linked_to_its_chat_media_file() {
        let payload = serde_json::json!({
            "friend1": [
                {
                    "From": "friend1",
                    "Media Type": "MEDIA",
                    "Created": "2026-07-07 22:44:03 UTC",
                    "Content": "",
                    "Conversation Title": null,
                    "IsSender": false,
                    "Created(microseconds)": 1783464243030i64,
                    "Media IDs": "b~EiQSFabc"
                }
            ]
        });
        let part_dir = part_dir_with_chat_history(&payload);
        let media_dir = part_dir.join("chat_media");
        fs::create_dir_all(&media_dir).unwrap();
        fs::write(media_dir.join("2026-07-07_b~EiQSFabc.jpg"), b"fake-jpg-bytes").unwrap();
        let job_dir = part_dir.parent().unwrap();

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();

        let summary = parse_chats_blocking(&conn, job_dir, &|_, _, _| {}).unwrap();
        assert_eq!(summary.media_assets_linked, 1);

        let (asset_media_type, asset_source_type): (String, String) = conn
            .query_row(
                "SELECT media_type, source_type FROM assets WHERE original_path LIKE '%b~EiQSFabc.jpg'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(asset_media_type, "image");
        assert_eq!(asset_source_type, "chat");

        let linked_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chat_message_assets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(linked_count, 1);

        // Re-ingestion must resolve to the same asset and message row, not
        // duplicate either.
        let second = parse_chats_blocking(&conn, job_dir, &|_, _, _| {}).unwrap();
        assert_eq!(second.media_assets_linked, 0);
        let asset_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(asset_count, 1);
    }

    #[test]
    fn media_message_without_a_matching_file_links_nothing() {
        let payload = serde_json::json!({
            "friend1": [
                {
                    "From": "friend1",
                    "Media Type": "MEDIA",
                    "Created": "2026-07-07 22:44:03 UTC",
                    "Content": "",
                    "Conversation Title": null,
                    "IsSender": false,
                    "Created(microseconds)": 1783464243030i64,
                    "Media IDs": "b~doesnotexist"
                }
            ]
        });
        let part_dir = part_dir_with_chat_history(&payload);
        let job_dir = part_dir.parent().unwrap();

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();

        let summary = parse_chats_blocking(&conn, job_dir, &|_, _, _| {}).unwrap();
        assert_eq!(summary.media_assets_linked, 0);
        assert_eq!(summary.messages_inserted, 1);
    }

    #[test]
    fn parses_one_to_one_and_group_threads() {
        let payload = serde_json::json!({
            "arturs_hermanis": [
                {
                    "From": "arturs_hermanis",
                    "Media Type": "TEXT",
                    "Created": "2026-07-07 22:44:03 UTC",
                    "Content": "hey",
                    "Conversation Title": null,
                    "IsSender": false,
                    "Created(microseconds)": 1783464243030i64,
                    "Media IDs": ""
                },
                {
                    "From": "d4nkm3m3sb0i",
                    "Media Type": "TEXT",
                    "Created": "2026-07-07 22:45:00 UTC",
                    "Content": "yo",
                    "Conversation Title": null,
                    "IsSender": true,
                    "Created(microseconds)": 1783464300000i64,
                    "Media IDs": ""
                }
            ],
            "ef2cf55a-group": [
                {
                    "From": "bressersteun",
                    "Media Type": "TEXT",
                    "Created": "2026-07-07 17:20:29 UTC",
                    "Content": null,
                    "Conversation Title": "Kkr tips",
                    "IsSender": false,
                    "Created(microseconds)": 1783444829126i64,
                    "Media IDs": ""
                }
            ]
        });
        let part_dir = part_dir_with_chat_history(&payload);
        let job_dir = part_dir.parent().unwrap();

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();

        let summary = parse_chats_blocking(&conn, job_dir, &|_, _, _| {}).unwrap();
        assert_eq!(summary.threads_found, 2);
        assert_eq!(summary.threads_inserted, 2);
        assert_eq!(summary.messages_inserted, 3);

        let (title, is_group): (Option<String>, i64) = conn
            .query_row(
                "SELECT title, is_group FROM chat_threads WHERE external_id = 'ef2cf55a-group'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, Some("Kkr tips".to_string()));
        assert_eq!(is_group, 1);

        let one_to_one_is_group: i64 = conn
            .query_row(
                "SELECT is_group FROM chat_threads WHERE external_id = 'arturs_hermanis'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(one_to_one_is_group, 0);
    }

    #[test]
    fn reingesting_the_same_export_does_not_duplicate_messages() {
        let payload = serde_json::json!({
            "friend1": [
                {
                    "From": "friend1",
                    "Media Type": "TEXT",
                    "Created": "2026-07-07 22:44:03 UTC",
                    "Content": "hey",
                    "Conversation Title": null,
                    "IsSender": false,
                    "Created(microseconds)": 1783464243030i64,
                    "Media IDs": ""
                }
            ]
        });
        let part_dir = part_dir_with_chat_history(&payload);
        let job_dir = part_dir.parent().unwrap();

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();

        parse_chats_blocking(&conn, job_dir, &|_, _, _| {}).unwrap();
        let second = parse_chats_blocking(&conn, job_dir, &|_, _, _| {}).unwrap();
        assert_eq!(second.threads_inserted, 0);
        assert_eq!(second.messages_inserted, 0);

        let message_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chat_messages", [], |row| row.get(0))
            .unwrap();
        assert_eq!(message_count, 1);
    }

    #[test]
    fn enriches_one_to_one_title_from_friends_json() {
        let chat_payload = serde_json::json!({
            "nickmarsmans": [
                {
                    "From": "nickmarsmans",
                    "Media Type": "TEXT",
                    "Created": "2026-07-07 22:44:03 UTC",
                    "Content": "hey",
                    "Conversation Title": null,
                    "IsSender": false,
                    "Created(microseconds)": 1783464243030i64,
                    "Media IDs": ""
                }
            ]
        });
        let part_dir = part_dir_with_chat_history(&chat_payload);
        let friends_payload = serde_json::json!({
            "Friends": [{ "Username": "nickmarsmans", "Display Name": "Nick" }]
        });
        fs::write(
            part_dir.join("json").join("friends.json"),
            serde_json::to_string(&friends_payload).unwrap(),
        )
        .unwrap();
        let job_dir = part_dir.parent().unwrap();

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        parse_chats_blocking(&conn, job_dir, &|_, _, _| {}).unwrap();

        let title: Option<String> = conn
            .query_row(
                "SELECT title FROM chat_threads WHERE external_id = 'nickmarsmans'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, Some("Nick".to_string()));
    }
}

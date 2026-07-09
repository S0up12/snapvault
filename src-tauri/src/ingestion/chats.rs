use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::memories::parse_snap_date;

#[derive(Clone, Serialize)]
pub struct ParseChatsSummary {
    pub threads_found: usize,
    pub threads_inserted: usize,
    pub messages_found: usize,
    pub messages_inserted: usize,
}

/// Reads `chat_history.json` (conversation id -> message list) and populates
/// `chat_threads`/`chat_messages`. A conversation is a group if any of its
/// messages carries a non-empty `Conversation Title` distinct from its own
/// id - Snapchat only ever sets that field for group chats. 1:1 threads with
/// no title get their counterpart's `friends.json` display name instead of
/// the raw username, when available. `snap_history.json` (ephemeral snaps)
/// is intentionally not parsed here - the schema anticipates it as a
/// separate `source`, but nothing in this app reads it yet.
pub(super) fn parse_chats_blocking(
    conn: &Connection,
    job_dir: &std::path::Path,
    emit: &dyn Fn(usize, usize, String),
) -> Result<ParseChatsSummary, String> {
    emit(0, 0, "Parsing chat history".to_string());

    let part_dirs = super::find_part_dirs(job_dir)?;
    let conversations = load_chat_history(&part_dirs)?;
    let friends_map = load_friends_display_names(&part_dirs)?;

    let total_messages: usize = conversations.values().map(|v| v.len()).sum();
    emit(
        0,
        total_messages,
        format!("Importing {} conversations", conversations.len()),
    );
    let emit_step = (total_messages / 100).max(1);

    let mut threads_inserted = 0usize;
    let mut messages_inserted = 0usize;
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
    })
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

use std::collections::HashMap;

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

use crate::db::DbState;

#[derive(Clone, Serialize)]
pub struct ChatThreadSummary {
    pub id: String,
    pub external_id: String,
    pub display_name: String,
    pub is_group: bool,
    pub message_count: i64,
    pub latest_at: Option<String>,
    pub latest_preview: String,
}

#[derive(Clone, Serialize)]
pub struct ChatMessageMedia {
    pub id: String,
    pub media_type: String,
    pub original_path: String,
    pub overlay_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub playback_path: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ChatMessageView {
    pub id: String,
    pub sender: String,
    pub sender_label: String,
    pub is_me: bool,
    pub body: Option<String>,
    pub sent_at: String,
    pub message_type: String,
    pub media: Vec<ChatMessageMedia>,
}

/// Lists every chat thread, newest activity first, each with a message count
/// and a one-line preview of its latest message (falling back to "Media
/// attachment" / "No messages yet" the same way the reference app's preview
/// text does).
#[tauri::command]
pub fn list_chat_threads(state: tauri::State<DbState>) -> Result<Vec<ChatThreadSummary>, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    load_threads(&conn)
}

fn load_threads(conn: &Connection) -> Result<Vec<ChatThreadSummary>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT
                t.id, t.external_id, t.title, t.is_group,
                (SELECT COUNT(*) FROM chat_messages WHERE thread_id = t.id),
                (SELECT sent_at FROM chat_messages WHERE thread_id = t.id ORDER BY sent_at DESC LIMIT 1),
                (SELECT body FROM chat_messages WHERE thread_id = t.id ORDER BY sent_at DESC LIMIT 1),
                (SELECT message_type FROM chat_messages WHERE thread_id = t.id ORDER BY sent_at DESC LIMIT 1),
                (SELECT sender FROM chat_messages WHERE thread_id = t.id ORDER BY sent_at DESC LIMIT 1),
                (SELECT raw_payload FROM chat_messages WHERE thread_id = t.id ORDER BY sent_at DESC LIMIT 1)
             FROM chat_threads t
             ORDER BY (SELECT sent_at FROM chat_messages WHERE thread_id = t.id ORDER BY sent_at DESC LIMIT 1) DESC",
        )
        .map_err(|err| err.to_string())?;

    let threads = stmt
        .query_map([], |row| {
            let external_id: String = row.get(1)?;
            let title: Option<String> = row.get(2)?;
            let latest_body: Option<String> = row.get(6)?;
            let latest_message_type: Option<String> = row.get(7)?;
            let latest_sender: Option<String> = row.get(8)?;
            let latest_raw_payload: Option<String> = row.get(9)?;

            let latest_is_me = latest_raw_payload
                .as_deref()
                .and_then(is_sender_from_raw_payload)
                .unwrap_or(false);

            Ok(ChatThreadSummary {
                id: row.get(0)?,
                external_id: external_id.clone(),
                display_name: title.unwrap_or(external_id),
                is_group: row.get::<_, i64>(3)? != 0,
                message_count: row.get(4)?,
                latest_at: row.get(5)?,
                latest_preview: message_preview(latest_body, latest_message_type, latest_sender, latest_is_me),
            })
        })
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    Ok(threads)
}

/// Lists every message in a thread, oldest first, resolving each message's
/// "is this me" flag from the raw Snapchat `IsSender` field captured at
/// ingestion time rather than guessing from the sender name.
#[tauri::command]
pub fn list_chat_messages(
    state: tauri::State<DbState>,
    thread_id: String,
) -> Result<Vec<ChatMessageView>, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    load_messages(&conn, &thread_id)
}

fn load_messages(conn: &Connection, thread_id: &str) -> Result<Vec<ChatMessageView>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, sender, body, sent_at, message_type, raw_payload
             FROM chat_messages
             WHERE thread_id = ?1
             ORDER BY sent_at ASC",
        )
        .map_err(|err| err.to_string())?;

    let mut messages = stmt
        .query_map([thread_id], |row| {
            let sender: String = row.get(1)?;
            let raw_payload: Option<String> = row.get(5)?;
            let is_me = raw_payload
                .as_deref()
                .and_then(is_sender_from_raw_payload)
                .unwrap_or(false);

            Ok(ChatMessageView {
                id: row.get(0)?,
                sender_label: if is_me { "Me".to_string() } else { sender.clone() },
                sender,
                is_me,
                body: row.get(2)?,
                sent_at: row.get(3)?,
                message_type: row.get(4)?,
                media: Vec::new(),
            })
        })
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;

    // Separate query rather than a JOIN on the message query above: a JOIN
    // would repeat every non-media column per attachment, which then needs
    // unwinding back into one row per message anyway.
    let mut media_stmt = conn
        .prepare(
            "SELECT cma.message_id, a.id, a.media_type, a.original_path, a.overlay_path, a.thumbnail_path, a.playback_path
             FROM chat_message_assets cma
             JOIN assets a ON a.id = cma.asset_id
             JOIN chat_messages cm ON cm.id = cma.message_id
             WHERE cm.thread_id = ?1",
        )
        .map_err(|err| err.to_string())?;
    let media_rows = media_stmt
        .query_map([thread_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                ChatMessageMedia {
                    id: row.get(1)?,
                    media_type: row.get(2)?,
                    original_path: row.get(3)?,
                    overlay_path: row.get(4)?,
                    thumbnail_path: row.get(5)?,
                    playback_path: row.get(6)?,
                },
            ))
        })
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;

    let index_by_id: HashMap<String, usize> = messages
        .iter()
        .enumerate()
        .map(|(idx, message)| (message.id.clone(), idx))
        .collect();
    for (message_id, media) in media_rows {
        if let Some(&idx) = index_by_id.get(&message_id) {
            messages[idx].media.push(media);
        }
    }

    // Drop messages with nothing displayable: no body text and no linked
    // media. This isn't limited to `TEXT` rows - Snapchat's chat_history.json
    // routinely references MEDIA/SHARE/STICKER/STATUS/NOTE entries whose
    // media either has no `Media IDs` at all or an id with no matching file
    // in the export (expired/premium snaps, media the export just didn't
    // bundle), so `media` ends up empty for those too. Without this filter
    // those render as a bare "Media attachment" bubble with nothing inside.
    messages.retain(|message| {
        message.body.as_deref().is_some_and(|body| !body.trim().is_empty()) || !message.media.is_empty()
    });

    Ok(messages)
}

fn is_sender_from_raw_payload(raw_payload: &str) -> Option<bool> {
    let value: Value = serde_json::from_str(raw_payload).ok()?;
    value.get("IsSender")?.as_bool()
}

fn message_preview(
    body: Option<String>,
    message_type: Option<String>,
    sender: Option<String>,
    is_me: bool,
) -> String {
    let trimmed = body.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let base = match trimmed {
        Some(text) => text.to_string(),
        None => match message_type.as_deref() {
            None => return "No messages yet".to_string(),
            Some("TEXT") => "No messages yet".to_string(),
            Some(_) => "Media attachment".to_string(),
        },
    };
    if sender.is_none() {
        return "No messages yet".to_string();
    }
    if is_me {
        format!("You: {base}")
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;

    fn seed_thread(conn: &Connection, id: &str, external_id: &str, title: Option<&str>, is_group: bool) {
        conn.execute(
            "INSERT INTO chat_threads (id, external_id, title, is_group) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, external_id, title, is_group as i64],
        )
        .unwrap();
    }

    fn seed_message(conn: &Connection, thread_id: &str, sender: &str, body: Option<&str>, sent_at: &str, is_sender: bool) {
        let raw_payload = serde_json::json!({ "IsSender": is_sender }).to_string();
        conn.execute(
            "INSERT INTO chat_messages (id, thread_id, sender, body, sent_at, message_type, source, dedupe_key, raw_payload)
             VALUES (?1, ?2, ?3, ?4, ?5, 'TEXT', 'chat_history', ?6, ?7)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                thread_id,
                sender,
                body,
                sent_at,
                format!("{thread_id}:{sender}:{sent_at}"),
                raw_payload,
            ],
        )
        .unwrap();
    }

    #[test]
    fn thread_preview_uses_latest_message_and_you_prefix() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        seed_message(&conn, "t1", "friend1", Some("hey"), "2020-01-01T00:00:00.000Z", false);
        seed_message(&conn, "t1", "me", Some("hi back"), "2020-01-02T00:00:00.000Z", true);

        let threads = load_threads(&conn).unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].message_count, 2);
        assert_eq!(threads[0].latest_preview, "You: hi back");
        assert_eq!(threads[0].display_name, "friend1");
    }

    #[test]
    fn thread_with_no_messages_reports_no_messages_yet() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);

        let threads = load_threads(&conn).unwrap();
        assert_eq!(threads[0].latest_preview, "No messages yet");
        assert_eq!(threads[0].message_count, 0);
    }

    #[test]
    fn media_message_without_body_shows_attachment_placeholder() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        conn.execute(
            "INSERT INTO chat_messages (id, thread_id, sender, body, sent_at, message_type, source, dedupe_key, raw_payload)
             VALUES ('m1', 't1', 'friend1', NULL, '2020-01-01T00:00:00.000Z', 'MEDIA', 'chat_history', 'dk1', '{\"IsSender\":false}')",
            [],
        )
        .unwrap();

        let threads = load_threads(&conn).unwrap();
        assert_eq!(threads[0].latest_preview, "Media attachment");
    }

    #[test]
    fn group_thread_uses_title_not_external_id() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "uuid-123", Some("Kkr tips"), true);

        let threads = load_threads(&conn).unwrap();
        assert_eq!(threads[0].display_name, "Kkr tips");
        assert!(threads[0].is_group);
    }

    #[test]
    fn messages_resolve_is_me_from_raw_payload_not_username_heuristic() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        seed_message(&conn, "t1", "some_weird_username", Some("hi"), "2020-01-01T00:00:00.000Z", true);

        let messages = load_messages(&conn, "t1").unwrap();
        assert!(messages[0].is_me);
        assert_eq!(messages[0].sender_label, "Me");
    }

    #[test]
    fn messages_are_ordered_oldest_first() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        seed_message(&conn, "t1", "friend1", Some("second"), "2020-01-02T00:00:00.000Z", false);
        seed_message(&conn, "t1", "friend1", Some("first"), "2020-01-01T00:00:00.000Z", false);

        let messages = load_messages(&conn, "t1").unwrap();
        assert_eq!(messages.iter().map(|m| m.body.clone().unwrap()).collect::<Vec<_>>(), vec!["first", "second"]);
    }

    #[test]
    fn messages_carry_their_linked_media() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        seed_message(&conn, "t1", "friend1", None, "2020-01-01T00:00:00.000Z", false);
        let message_id: String = conn
            .query_row("SELECT id FROM chat_messages WHERE thread_id = 't1'", [], |row| row.get(0))
            .unwrap();

        conn.execute(
            "INSERT INTO assets (id, source_type, media_type, original_path) VALUES ('a1', 'chat', 'image', '/chat_media/a.jpg')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_message_assets (message_id, asset_id) VALUES (?1, 'a1')",
            [&message_id],
        )
        .unwrap();

        let messages = load_messages(&conn, "t1").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].media.len(), 1);
        assert_eq!(messages[0].media[0].original_path, "/chat_media/a.jpg");
    }

    #[test]
    fn drops_empty_text_messages_with_no_body_and_no_media() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        // Empty TEXT row - nothing to display, should be dropped.
        seed_message(&conn, "t1", "friend1", None, "2020-01-01T00:00:00.000Z", false);
        // Real message - should survive.
        seed_message(&conn, "t1", "friend1", Some("hi"), "2020-01-02T00:00:00.000Z", false);

        let messages = load_messages(&conn, "t1").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body.as_deref(), Some("hi"));
    }

    #[test]
    fn drops_non_text_messages_with_no_body_and_no_linked_media() {
        // Snapchat's chat_history.json routinely references MEDIA/SHARE/
        // STICKER/STATUS/NOTE entries whose media never resolves to a file
        // in the export (an id with nothing at that path, or no Media IDs at
        // all). With no body either, there is genuinely nothing to display -
        // this used to render as a bare "Media attachment" bubble with no
        // media inside it.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        for (id, message_type) in [
            ("m1", "MEDIA"),
            ("m2", "SHARE"),
            ("m3", "STICKER"),
            ("m4", "STATUS"),
            ("m5", "NOTE"),
        ] {
            conn.execute(
                "INSERT INTO chat_messages (id, thread_id, sender, body, sent_at, message_type, source, dedupe_key, raw_payload)
                 VALUES (?1, 't1', 'friend1', NULL, '2020-01-01T00:00:00.000Z', ?2, 'chat_history', ?3, '{\"IsSender\":false}')",
                rusqlite::params![id, message_type, format!("dk-{id}")],
            )
            .unwrap();
        }

        let messages = load_messages(&conn, "t1").unwrap();
        assert!(messages.is_empty());
    }

    #[test]
    fn keeps_non_text_messages_that_have_a_caption_or_linked_media() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        // A MEDIA message with a caption but no linked media file - the
        // caption is real content, so it must survive even with no media.
        conn.execute(
            "INSERT INTO chat_messages (id, thread_id, sender, body, sent_at, message_type, source, dedupe_key, raw_payload)
             VALUES ('m1', 't1', 'friend1', 'check this out', '2020-01-01T00:00:00.000Z', 'MEDIA', 'chat_history', 'dk1', '{\"IsSender\":false}')",
            [],
        )
        .unwrap();
        // A MEDIA message with no body but a successfully linked asset.
        conn.execute(
            "INSERT INTO chat_messages (id, thread_id, sender, body, sent_at, message_type, source, dedupe_key, raw_payload)
             VALUES ('m2', 't1', 'friend1', NULL, '2020-01-02T00:00:00.000Z', 'MEDIA', 'chat_history', 'dk2', '{\"IsSender\":false}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets (id, source_type, media_type, original_path) VALUES ('a1', 'chat', 'image', '/chat_media/a.jpg')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_message_assets (message_id, asset_id) VALUES ('m2', 'a1')",
            [],
        )
        .unwrap();

        let messages = load_messages(&conn, "t1").unwrap();
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn message_without_linked_media_has_empty_media_list() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        seed_message(&conn, "t1", "friend1", Some("hi"), "2020-01-01T00:00:00.000Z", false);

        let messages = load_messages(&conn, "t1").unwrap();
        assert!(messages[0].media.is_empty());
    }
}

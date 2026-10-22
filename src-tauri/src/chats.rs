use std::collections::HashMap;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
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

#[derive(Clone, Serialize, Deserialize)]
pub struct ChatMessageMedia {
    pub id: String,
    pub media_type: String,
    pub original_path: String,
    pub overlay_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub playback_path: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
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

const DEFAULT_PAGE: u32 = 100;
const MAX_PAGE: u32 = 500;
const DEFAULT_AROUND: u32 = 60;

#[derive(Clone, Serialize, Deserialize)]
pub struct ChatMessagesPage {
    /// Chronological (oldest first) within the page.
    pub messages: Vec<ChatMessageView>,
    /// Older messages exist (legacy name; same as `has_more_before`).
    pub has_more: bool,
    /// Opaque cursor: pass as `before` to load the next older page. None when `has_more` is false.
    pub next_before: Option<String>,
    #[serde(default)]
    pub has_more_before: bool,
    /// Newer messages exist (false for the newest page).
    #[serde(default)]
    pub has_more_after: bool,
    /// Opaque cursor: pass as `after` to load the next newer page. None when `has_more_after` is false.
    #[serde(default)]
    pub next_after: Option<String>,
}

/// Lists one page of a thread's messages. Without cursors returns the newest
/// page; `before` walks back in time, `after` walks forward (chronological),
/// `around` (a message id) returns a window centred on that message. `is_me`
/// comes from the denormalised `is_sender` column (raw `IsSender` as fallback).
#[tauri::command]
pub fn list_chat_messages(
    app: tauri::AppHandle,
    state: tauri::State<DbState>,
    thread_id: String,
    before: Option<String>,
    limit: Option<u32>,
    around: Option<String>,
    after: Option<String>,
) -> Result<ChatMessagesPage, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    let mut page = load_window(&conn, &thread_id, before.as_deref(), after.as_deref(), around.as_deref(), limit)?;
    // DB paths are media-root-relative; the frontend gets absolute ones.
    let root = crate::storage::resolve_media_root(&app)?;
    let abs = |p: &str| crate::storage::to_absolute(&root, p).display().to_string();
    for m in page.messages.iter_mut().flat_map(|m| m.media.iter_mut()) {
        m.original_path = abs(&m.original_path);
        m.overlay_path = m.overlay_path.as_deref().map(abs);
        m.thumbnail_path = m.thumbnail_path.as_deref().map(abs);
        m.playback_path = m.playback_path.as_deref().map(abs);
    }
    Ok(page)
}

#[cfg(test)]
fn load_messages(
    conn: &Connection,
    thread_id: &str,
    before: Option<&str>,
    limit: Option<u32>,
) -> Result<ChatMessagesPage, String> {
    load_window(conn, thread_id, before, None, None, limit)
}

/// Cursor = "<sent_at>|<id>"; keyset on (sent_at, id) so equal timestamps
/// paginate stably.
fn parse_cursor(cursor: &str) -> Result<(String, String), String> {
    let (s, i) = cursor.split_once('|').ok_or("invalid cursor")?;
    Ok((s.to_string(), i.to_string()))
}

type CursorRow = (String, ChatMessageView);

/// Fetches up to `take` displayable rows strictly before (or, if `inclusive`,
/// at-or-before) the cursor, newest first then reversed to chronological; or
/// with `newer`, strictly after it, oldest first. Returns the rows plus
/// whether more exist further in that direction.
fn fetch_rows(
    conn: &Connection,
    thread_id: &str,
    cursor: Option<&(String, String)>,
    newer: bool,
    inclusive: bool,
    take: usize,
) -> Result<(Vec<CursorRow>, bool), String> {
    let (op, dir) = match (newer, inclusive) {
        (true, _) => (">", "ASC"),
        (false, true) => ("<=", "DESC"),
        (false, false) => ("<", "DESC"),
    };
    // Rows with nothing displayable (no body, no media) are filtered here so
    // pages stay full; see the exact whitespace check in `attach_media_and_filter`.
    let mut stmt = conn
        .prepare(&format!(
            "SELECT id, sender, body, sent_at, message_type, is_sender,
                    CASE WHEN is_sender IS NULL THEN raw_payload END
             FROM chat_messages
             WHERE thread_id = ?1
               AND (?2 IS NULL OR (sent_at, id) {op} (?2, ?3))
               AND ((body IS NOT NULL AND body <> '')
                    OR EXISTS (SELECT 1 FROM chat_message_assets WHERE message_id = chat_messages.id))
             ORDER BY sent_at {dir}, id {dir}
             LIMIT ?4"
        ))
        .map_err(|err| err.to_string())?;
    let mut rows = stmt
        .query_map(
            rusqlite::params![thread_id, cursor.map(|c| &c.0), cursor.map(|c| &c.1), take as i64 + 1],
            |row| {
                let sender: String = row.get(1)?;
                let is_sender: Option<i64> = row.get(5)?;
                let raw_payload: Option<String> = row.get(6)?;
                let is_me = match is_sender {
                    Some(v) => v != 0,
                    None => raw_payload.as_deref().and_then(is_sender_from_raw_payload).unwrap_or(false),
                };
                let id: String = row.get(0)?;
                let sent_at: String = row.get(3)?;
                Ok((
                    format!("{sent_at}|{id}"),
                    ChatMessageView {
                        id,
                        sender_label: if is_me { "Me".to_string() } else { sender.clone() },
                        sender,
                        is_me,
                        body: row.get(2)?,
                        sent_at,
                        message_type: row.get(4)?,
                        media: Vec::new(),
                    },
                ))
            },
        )
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    let more = rows.len() > take;
    rows.truncate(take);
    if !newer {
        rows.reverse();
    }
    Ok((rows, more))
}

fn load_window(
    conn: &Connection,
    thread_id: &str,
    before: Option<&str>,
    after: Option<&str>,
    around: Option<&str>,
    limit: Option<u32>,
) -> Result<ChatMessagesPage, String> {
    let default = if around.is_some() { DEFAULT_AROUND } else { DEFAULT_PAGE };
    let limit = limit.unwrap_or(default).clamp(1, MAX_PAGE) as usize;

    let (rows, more_before, more_after) = if let Some(target) = around {
        let sent_at: String = conn
            .query_row(
                "SELECT sent_at FROM chat_messages WHERE id = ?1 AND thread_id = ?2",
                rusqlite::params![target, thread_id],
                |r| r.get(0),
            )
            .map_err(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => "message not found in this thread".to_string(),
                other => other.to_string(),
            })?;
        let cur = (sent_at, target.to_string());
        // limit 60 -> 30 before + target + 29 after.
        let before_n = limit / 2;
        let after_n = limit - 1 - before_n;
        let (mut older, more_before) = fetch_rows(conn, thread_id, Some(&cur), false, true, before_n + 1)?;
        let (newer, more_after) = fetch_rows(conn, thread_id, Some(&cur), true, false, after_n)?;
        older.extend(newer);
        (older, more_before, more_after)
    } else if let Some(cursor) = after {
        let cur = parse_cursor(cursor)?;
        let (rows, more_after) = fetch_rows(conn, thread_id, Some(&cur), true, false, limit)?;
        // The cursor row itself is older than everything returned.
        (rows, true, more_after)
    } else {
        let cur = before.map(parse_cursor).transpose()?;
        let (rows, more_before) = fetch_rows(conn, thread_id, cur.as_ref(), false, false, limit)?;
        // With a `before` cursor the cursor row is newer than the page.
        (rows, more_before, cur.is_some())
    };

    // Cursors come from the unfiltered rows so the whitespace filter below
    // cannot move them.
    let next_before = if more_before { rows.first().map(|r| r.0.clone()) } else { None };
    let next_after = if more_after { rows.last().map(|r| r.0.clone()) } else { None };
    let mut messages: Vec<ChatMessageView> = rows.into_iter().map(|r| r.1).collect();
    attach_media_and_filter(conn, &mut messages)?;
    Ok(ChatMessagesPage {
        messages,
        has_more: more_before,
        next_before,
        has_more_before: more_before,
        has_more_after: more_after,
        next_after,
    })
}

/// Forward page (oldest first) of displayable messages strictly after `cursor`
/// (None = from the start), same rows/filter as `list_chat_messages`. Returns the
/// messages and the cursor for the next call (None when exhausted). Used by export.
pub(crate) fn messages_after(
    conn: &Connection,
    thread_id: &str,
    cursor: Option<&str>,
    take: usize,
) -> Result<(Vec<ChatMessageView>, Option<String>), String> {
    let cur = cursor.map(parse_cursor).transpose()?;
    let (rows, more) = fetch_rows(conn, thread_id, cur.as_ref(), true, false, take)?;
    let next = if more { rows.last().map(|r| r.0.clone()) } else { None };
    let mut messages: Vec<ChatMessageView> = rows.into_iter().map(|r| r.1).collect();
    attach_media_and_filter(conn, &mut messages)?;
    Ok((messages, next))
}

fn attach_media_and_filter(conn: &Connection, messages: &mut Vec<ChatMessageView>) -> Result<(), String> {
    // Media for this page only. Separate query rather than a JOIN: a JOIN
    // would repeat every non-media column per attachment.
    let placeholders = vec!["?"; messages.len()].join(",");
    let mut media_stmt = conn
        .prepare(&format!(
            "SELECT cma.message_id, a.id, a.media_type, a.original_path, a.overlay_path, a.thumbnail_path, a.playback_path
             FROM chat_message_assets cma
             JOIN assets a ON a.id = cma.asset_id
             WHERE cma.message_id IN ({placeholders})"
        ))
        .map_err(|err| err.to_string())?;
    let media_rows = media_stmt
        .query_map(rusqlite::params_from_iter(messages.iter().map(|m| &m.id)), |row| {
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

    // Whitespace-only bodies with no media are not displayable (the SQL
    // prefilter only drops NULL/empty). Snapchat's chat_history.json routinely
    // references MEDIA/SHARE/STICKER/... entries with no resolvable file;
    // without this they render as bare "Media attachment" bubbles.
    messages.retain(|message| {
        message.body.as_deref().is_some_and(|body| !body.trim().is_empty()) || !message.media.is_empty()
    });
    Ok(())
}

// ---------------------------------------------------------------- search

const SEARCH_DEFAULT: u32 = 50;
const SEARCH_MAX: u32 = 200;
const SEARCH_TOTAL_CAP: i64 = 1000;

#[derive(Clone, Serialize, Deserialize)]
pub struct ChatSearchHit {
    pub message_id: String,
    pub thread_id: String,
    pub thread_title: Option<String>,
    pub sent_at: String,
    pub is_sender: bool,
    /// "Me" for own messages, else the sender name.
    pub sender_label: String,
    /// Text with matched terms wrapped in U+0001 ... U+0002.
    pub snippet: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChatSearchPage {
    pub hits: Vec<ChatSearchHit>,
    /// min(real count, 1000).
    pub total: i64,
    pub total_capped: bool,
    pub has_more: bool,
}

/// Turns free text into a safe FTS5 expression: whitespace-split tokens, each
/// double-quoted (embedded quotes doubled) so operators/columns/NEAR/* are
/// literals; implicit AND; the last token gets a prefix `*`. Tokens with no
/// alphanumeric char (punctuation, emoji: the unicode61 tokenizer drops them)
/// are discarded. None when nothing searchable remains.
fn sanitize_fts_query(query: &str) -> Option<String> {
    let mut tokens: Vec<String> = query
        .split_whitespace()
        .filter(|t| t.chars().any(char::is_alphanumeric))
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    tokens.last_mut()?.push('*');
    Some(tokens.join(" "))
}

/// Full-text search over chat message bodies (never logs the query).
#[tauri::command]
pub fn search_chat_messages(
    state: tauri::State<DbState>,
    query: String,
    thread_id: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<ChatSearchPage, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    search_messages(&conn, &query, thread_id.as_deref(), limit, offset)
}

fn search_messages(
    conn: &Connection,
    query: &str,
    thread_id: Option<&str>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<ChatSearchPage, String> {
    let empty = ChatSearchPage { hits: Vec::new(), total: 0, total_capped: false, has_more: false };
    let Some(expr) = sanitize_fts_query(query) else { return Ok(empty) };
    let limit = limit.unwrap_or(SEARCH_DEFAULT).clamp(1, SEARCH_MAX) as i64;
    let offset = offset.unwrap_or(0) as i64;

    let counted: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (
                SELECT 1 FROM chat_messages_fts f JOIN chat_messages m ON m.rowid = f.rowid
                WHERE chat_messages_fts MATCH ?1 AND (?2 IS NULL OR m.thread_id = ?2) LIMIT ?3)",
            rusqlite::params![expr, thread_id, SEARCH_TOTAL_CAP + 1],
            |r| r.get(0),
        )
        .map_err(|err| err.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT m.id, m.thread_id, t.title, m.sent_at, m.is_sender,
                    CASE WHEN m.is_sender IS NULL THEN m.raw_payload END, m.sender,
                    snippet(chat_messages_fts, 0, char(1), char(2), '…', 12)
             FROM chat_messages_fts f
             JOIN chat_messages m ON m.rowid = f.rowid
             JOIN chat_threads t ON t.id = m.thread_id
             WHERE chat_messages_fts MATCH ?1 AND (?2 IS NULL OR m.thread_id = ?2)
             ORDER BY bm25(chat_messages_fts), m.sent_at DESC, m.id DESC
             LIMIT ?3 OFFSET ?4",
        )
        .map_err(|err| err.to_string())?;
    let mut hits = stmt
        .query_map(rusqlite::params![expr, thread_id, limit + 1, offset], |row| {
            let is_sender: Option<i64> = row.get(4)?;
            let raw_payload: Option<String> = row.get(5)?;
            let is_me = match is_sender {
                Some(v) => v != 0,
                None => raw_payload.as_deref().and_then(is_sender_from_raw_payload).unwrap_or(false),
            };
            let sender: String = row.get(6)?;
            Ok(ChatSearchHit {
                message_id: row.get(0)?,
                thread_id: row.get(1)?,
                thread_title: row.get(2)?,
                sent_at: row.get(3)?,
                is_sender: is_me,
                sender_label: if is_me { "Me".to_string() } else { sender },
                snippet: row.get(7)?,
            })
        })
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    let has_more = hits.len() as i64 > limit;
    hits.truncate(limit as usize);
    Ok(ChatSearchPage {
        hits,
        total: counted.min(SEARCH_TOTAL_CAP),
        total_capped: counted > SEARCH_TOTAL_CAP,
        has_more,
    })
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

    fn ids(page: &ChatMessagesPage) -> Vec<String> {
        page.messages.iter().map(|m| m.body.clone().unwrap()).collect()
    }

    #[test]
    fn pagination_newest_first_page_chronological_with_boundaries() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        for i in 0..5 {
            seed_message(&conn, "t1", "f", Some(&format!("m{i}")), &format!("2020-01-0{}T00:00:00.000Z", i + 1), false);
        }
        let p1 = load_messages(&conn, "t1", None, Some(2)).unwrap();
        assert_eq!(ids(&p1), vec!["m3", "m4"]);
        assert!(p1.has_more);
        let p2 = load_messages(&conn, "t1", p1.next_before.as_deref(), Some(2)).unwrap();
        assert_eq!(ids(&p2), vec!["m1", "m2"]);
        assert!(p2.has_more);
        // Last page is exactly the remainder: has_more false, no cursor.
        let p3 = load_messages(&conn, "t1", p2.next_before.as_deref(), Some(2)).unwrap();
        assert_eq!(ids(&p3), vec!["m0"]);
        assert!(!p3.has_more && p3.next_before.is_none());
        // Exact fit: limit == total.
        let all = load_messages(&conn, "t1", None, Some(5)).unwrap();
        assert_eq!(all.messages.len(), 5);
        assert!(!all.has_more);
        // Limit is clamped, bad cursor rejected.
        assert_eq!(load_messages(&conn, "t1", None, Some(0)).unwrap().messages.len(), 1);
        assert!(load_messages(&conn, "t1", Some("garbage"), None).is_err());
    }

    #[test]
    fn pagination_is_stable_for_equal_timestamps() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        for i in 0..7 {
            conn.execute(
                "INSERT INTO chat_messages (id, thread_id, sender, body, sent_at, message_type, source, dedupe_key)
                 VALUES (?1, 't1', 'f', ?1, '2020-01-01T00:00:00.000Z', 'TEXT', 'chat_history', ?1)",
                [format!("id{i}")],
            )
            .unwrap();
        }
        let mut seen = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = load_messages(&conn, "t1", cursor.as_deref(), Some(3)).unwrap();
            let mut got = ids(&page);
            got.extend(seen);
            seen = got;
            if !page.has_more {
                break;
            }
            cursor = page.next_before;
        }
        assert_eq!(seen, (0..7).map(|i| format!("id{i}")).collect::<Vec<_>>());
    }

    #[test]
    fn is_sender_column_wins_and_null_falls_back_to_raw_payload() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        // seed_message leaves is_sender NULL: payload says me.
        seed_message(&conn, "t1", "x", Some("null-col"), "2020-01-01T00:00:00.000Z", true);
        // Column set, payload disagrees: column wins.
        conn.execute(
            "INSERT INTO chat_messages (id, thread_id, sender, body, sent_at, message_type, source, dedupe_key, raw_payload, is_sender)
             VALUES ('m2', 't1', 'x', 'col', '2020-01-02T00:00:00.000Z', 'TEXT', 'chat_history', 'dk2', '{\"IsSender\":true}', 0)",
            [],
        )
        .unwrap();
        let page = load_messages(&conn, "t1", None, None).unwrap();
        assert!(page.messages[0].is_me);
        assert!(!page.messages[1].is_me);
    }

    #[test]
    fn bench_50k_thread() {
        let path = std::env::temp_dir().join(format!("snapvault-bench-{}.db", uuid::Uuid::new_v4()));
        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch(crate::db::PRAGMAS).unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        let tx = conn.transaction().unwrap();
        for i in 0..50_000 {
            seed_message(&tx, "t1", "friend1", Some("hello"), &format!("2020-01-01T00:{:02}:{:02}.{:03}Z", i / 60000 % 60, i / 1000 % 60, i % 1000), i % 2 == 0);
        }
        tx.commit().unwrap();
        let start = std::time::Instant::now();
        let n = load_messages(&conn, "t1", None, Some(100)).unwrap().messages.len();
        println!("BENCH list_chat_messages 50k: {:?} ({n} messages)", start.elapsed());
        drop(conn);
        let _ = std::fs::remove_file(&path);
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

        let messages = load_messages(&conn, "t1", None, None).unwrap().messages;
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

        let messages = load_messages(&conn, "t1", None, None).unwrap().messages;
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

        let messages = load_messages(&conn, "t1", None, None).unwrap().messages;
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

        let messages = load_messages(&conn, "t1", None, None).unwrap().messages;
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

        let messages = load_messages(&conn, "t1", None, None).unwrap().messages;
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

        let messages = load_messages(&conn, "t1", None, None).unwrap().messages;
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn message_without_linked_media_has_empty_media_list() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed_thread(&conn, "t1", "friend1", None, false);
        seed_message(&conn, "t1", "friend1", Some("hi"), "2020-01-01T00:00:00.000Z", false);

        let messages = load_messages(&conn, "t1", None, None).unwrap().messages;
        assert!(messages[0].media.is_empty());
    }

    // ------------------------------------------------------------ FTS / search / windows

    fn fts_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn, crate::db::MIGRATIONS, std::path::Path::new("/none")).unwrap();
        conn
    }

    fn put(conn: &Connection, id: &str, thread: &str, body: Option<&str>, sent_at: &str) {
        conn.execute(
            "INSERT INTO chat_messages (id, thread_id, sender, body, sent_at, dedupe_key, is_sender)
             VALUES (?1, ?2, 'friend', ?3, ?4, ?1, 0)",
            rusqlite::params![id, thread, body, sent_at],
        )
        .unwrap();
    }

    fn fts_ids(conn: &Connection, q: &str) -> Vec<String> {
        conn.prepare("SELECT m.id FROM chat_messages_fts f JOIN chat_messages m ON m.rowid = f.rowid WHERE chat_messages_fts MATCH ?1 ORDER BY m.id")
            .unwrap()
            .query_map([q], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    fn fts_ok(conn: &Connection) {
        conn.execute_batch("INSERT INTO chat_messages_fts(chat_messages_fts) VALUES('integrity-check')").unwrap();
    }

    #[test]
    fn fts_triggers_follow_insert_update_delete() {
        let conn = fts_conn();
        seed_thread(&conn, "ft1", "e-ft1", None, false);
        put(&conn, "ft1-a", "ft1", Some("pizza tonight"), "2020-01-01T00:00:00.000Z");
        put(&conn, "ft1-b", "ft1", Some(""), "2020-01-02T00:00:00.000Z");
        put(&conn, "ft1-c", "ft1", None, "2020-01-03T00:00:00.000Z");
        assert_eq!(fts_ids(&conn, "pizza"), vec!["ft1-a"]);
        let docs = |c: &Connection| -> i64 { c.query_row("SELECT COUNT(*) FROM chat_messages_fts_docsize", [], |r| r.get(0)).unwrap() };
        assert_eq!(docs(&conn), 1); // empty/NULL bodies not indexed

        // update body: old terms gone, new terms found; updated_at trigger must not break it
        conn.execute("UPDATE chat_messages SET body = 'sushi' WHERE id = 'ft1-a'", []).unwrap();
        assert!(fts_ids(&conn, "pizza").is_empty());
        assert_eq!(fts_ids(&conn, "sushi"), vec!["ft1-a"]);
        // empty -> text, text -> empty
        conn.execute("UPDATE chat_messages SET body = 'ramen' WHERE id = 'ft1-b'", []).unwrap();
        conn.execute("UPDATE chat_messages SET body = NULL WHERE id = 'ft1-a'", []).unwrap();
        assert_eq!(fts_ids(&conn, "ramen"), vec!["ft1-b"]);
        assert!(fts_ids(&conn, "sushi").is_empty());
        // non-body update leaves index alone
        conn.execute("UPDATE chat_messages SET sender = 'zed' WHERE id = 'ft1-b'", []).unwrap();
        assert_eq!(fts_ids(&conn, "ramen"), vec!["ft1-b"]);
        // delete (also via thread cascade)
        conn.execute("DELETE FROM chat_messages WHERE id = 'ft1-c'", []).unwrap();
        assert_eq!(docs(&conn), 1);
        conn.execute("DELETE FROM chat_threads WHERE id = 'ft1'", []).unwrap();
        assert!(fts_ids(&conn, "ramen").is_empty());
        assert_eq!(docs(&conn), 0);
        fts_ok(&conn);
    }

    #[test]
    fn sanitizer_quotes_tokens_and_prefixes_last() {
        assert_eq!(sanitize_fts_query("hello world").as_deref(), Some("\"hello\" \"world\"*"));
        assert_eq!(sanitize_fts_query("  a   b\t").as_deref(), Some("\"a\" \"b\"*"));
        assert_eq!(sanitize_fts_query("say \"hi\"").as_deref(), Some("\"say\" \"\"\"hi\"\"\"*"));
        assert_eq!(sanitize_fts_query("a AND b OR NEAR").as_deref(), Some("\"a\" \"AND\" \"b\" \"OR\" \"NEAR\"*"));
        assert_eq!(sanitize_fts_query("caf\u{e9} na\u{ef}ve").as_deref(), Some("\"caf\u{e9}\" \"na\u{ef}ve\"*"));
        assert_eq!(sanitize_fts_query("hi *").as_deref(), Some("\"hi\"*"));
        for empty in ["", "   ", "\n\t", "*", "- ^ ( )", "\u{1f600}", "\"\""] {
            assert_eq!(sanitize_fts_query(empty), None, "{empty:?}");
        }
    }

    #[test]
    fn hostile_queries_never_error() {
        let conn = fts_conn();
        seed_thread(&conn, "hq1", "e-hq1", None, false);
        put(&conn, "hq1-a", "hq1", Some("and or near body:x caf\u{e9} \u{1f600} party"), "2020-01-01T00:00:00.000Z");
        for q in [
            "AND", "OR", "NOT", "NEAR(a b)", "a NEAR/2 b", "body:x", "{body}: x", "^x", "\"", "\"\"\"", "(", ")", "a*", "*",
            "-x", "x -y", "'; DROP TABLE chat_messages; --", "\u{1f600}", "caf\u{e9}", "CAFE", "\u{0}", "a\u{1}b",
        ] {
            search_messages(&conn, q, None, None, None).unwrap_or_else(|e| panic!("{q:?}: {e}"));
        }
        // operators are literal terms: "and or" matches the message containing both words
        assert_eq!(search_messages(&conn, "and or", None, None, None).unwrap().total, 1);
        assert_eq!(search_messages(&conn, "NEAR", None, None, None).unwrap().total, 1);
        // diacritics folded both ways, case-insensitive
        assert_eq!(search_messages(&conn, "CAFE", None, None, None).unwrap().total, 1);
        // empty -> empty page, not an error
        let p = search_messages(&conn, "   ", None, None, None).unwrap();
        assert!(p.hits.is_empty() && p.total == 0 && !p.has_more && !p.total_capped);
    }

    #[test]
    fn search_prefix_and_semantics_ranking_scope_and_paging() {
        let conn = fts_conn();
        seed_thread(&conn, "sr1", "e-sr1", Some("Alice"), false);
        seed_thread(&conn, "sr2", "e-sr2", None, false);
        put(&conn, "sr-a", "sr1", Some("pineapple on pizza is great"), "2020-01-01T00:00:00.000Z");
        put(&conn, "sr-b", "sr1", Some("pizza pizza pizza"), "2020-01-02T00:00:00.000Z");
        put(&conn, "sr-c", "sr2", Some("pizza"), "2020-01-03T00:00:00.000Z");
        put(&conn, "sr-d", "sr2", Some("pizza"), "2020-01-04T00:00:00.000Z");
        let s = |q: &str, t: Option<&str>| search_messages(&conn, q, t, None, None).unwrap();
        // prefix on last token only; AND semantics
        assert_eq!(s("piz", None).total, 4);
        assert_eq!(s("pizz gre", None).total, 0); // "pizz" not prefix-matched when not last
        assert_eq!(s("pizza gre", None).total, 1);
        assert_eq!(s("pizza pine", None).hits[0].message_id, "sr-a");
        // ranking: higher term frequency first (bm25); equal bm25 -> recency (sr-d before sr-c)
        let ids: Vec<_> = s("pizza", None).hits.iter().map(|h| h.message_id.clone()).collect();
        assert_eq!(ids, vec!["sr-b", "sr-d", "sr-c", "sr-a"]);
        // thread scope + metadata
        let scoped = s("pizza", Some("sr1"));
        assert_eq!(scoped.total, 2);
        assert_eq!(scoped.hits[0].thread_title.as_deref(), Some("Alice"));
        assert_eq!(scoped.hits[0].sender_label, "friend");
        assert!(!scoped.hits[0].is_sender);
        assert_eq!(s("pizza", Some("sr2")).hits[0].thread_title, None);
        // paging
        let p1 = search_messages(&conn, "pizza", None, Some(3), None).unwrap();
        assert!(p1.has_more && p1.hits.len() == 3 && p1.total == 4);
        let p2 = search_messages(&conn, "pizza", None, Some(3), Some(3)).unwrap();
        assert!(!p2.has_more && p2.hits.len() == 1 && p2.hits[0].message_id == "sr-a");
    }

    #[test]
    fn search_snippet_wraps_matches_in_markers() {
        let conn = fts_conn();
        seed_thread(&conn, "sn1", "e-sn1", None, false);
        let long = format!("{} needle {}", "word ".repeat(40), "tail ".repeat(40));
        put(&conn, "sn-a", "sn1", Some(&long), "2020-01-01T00:00:00.000Z");
        put(&conn, "sn-b", "sn1", Some("Hello big World"), "2020-01-02T00:00:00.000Z");
        let hit = &search_messages(&conn, "need", None, None, None).unwrap().hits[0];
        assert!(hit.snippet.contains("\u{1}needle\u{2}"), "{}", hit.snippet);
        assert!(hit.snippet.contains('…') && hit.snippet.len() < long.len());
        let hit = &search_messages(&conn, "hello world", None, None, None).unwrap().hits[0];
        assert_eq!(hit.snippet, "\u{1}Hello\u{2} big \u{1}World\u{2}");
    }

    fn window_conn(thread: &str, n: usize) -> Connection {
        let conn = fts_conn();
        seed_thread(&conn, thread, &format!("e-{thread}"), None, false);
        for i in 0..n {
            put(&conn, &format!("{thread}-{i:03}"), thread, Some(&format!("m{i}")), &format!("2020-01-01T00:{:02}:{:02}.000Z", i / 60, i % 60));
        }
        conn
    }

    fn bodies(p: &ChatMessagesPage) -> Vec<usize> {
        p.messages.iter().map(|m| m.body.as_ref().unwrap()[1..].parse().unwrap()).collect()
    }

    #[test]
    fn around_window_centers_and_reports_both_directions() {
        let conn = window_conn("aw1", 100);
        let p = load_window(&conn, "aw1", None, None, Some("aw1-050"), None).unwrap();
        assert_eq!(bodies(&p), (20..=79).collect::<Vec<_>>()); // 30 before, target, 29 after
        assert!(p.has_more_before && p.has_more && p.has_more_after);
        assert_eq!(p.next_before.as_deref(), Some("2020-01-01T00:00:20.000Z|aw1-020"));
        assert_eq!(p.next_after.as_deref(), Some("2020-01-01T00:01:19.000Z|aw1-079"));
        // walking both ways from the window is gap-free
        let older = load_window(&conn, "aw1", p.next_before.as_deref(), None, None, Some(5)).unwrap();
        assert_eq!(bodies(&older), vec![15, 16, 17, 18, 19]);
        let newer = load_window(&conn, "aw1", None, p.next_after.as_deref(), None, Some(5)).unwrap();
        assert_eq!(bodies(&newer), vec![80, 81, 82, 83, 84]);
        assert!(newer.has_more_after && newer.has_more_before);
        assert_eq!(newer.next_after.as_deref(), Some("2020-01-01T00:01:24.000Z|aw1-084"));
        // custom limit
        let p = load_window(&conn, "aw1", None, None, Some("aw1-050"), Some(5)).unwrap();
        assert_eq!(bodies(&p), vec![48, 49, 50, 51, 52]);
    }

    #[test]
    fn around_boundaries_first_last_single_and_wrong_thread() {
        let conn = window_conn("ab1", 100);
        let first = load_window(&conn, "ab1", None, None, Some("ab1-000"), None).unwrap();
        assert_eq!(bodies(&first), (0..=29).collect::<Vec<_>>());
        assert!(!first.has_more_before && first.next_before.is_none() && first.has_more_after);
        let last = load_window(&conn, "ab1", None, None, Some("ab1-099"), None).unwrap();
        assert_eq!(bodies(&last), (69..=99).collect::<Vec<_>>());
        assert!(last.has_more_before && !last.has_more_after && last.next_after.is_none());
        // exact fit at the edges: no false has_more
        let p = load_window(&conn, "ab1", None, None, Some("ab1-030"), None).unwrap();
        assert_eq!(p.messages.len(), 60);
        assert!(!p.has_more_before && p.next_before.is_none());

        seed_thread(&conn, "ab2", "e-ab2", None, false);
        put(&conn, "ab2-only", "ab2", Some("m0"), "2021-01-01T00:00:00.000Z");
        let one = load_window(&conn, "ab2", None, None, Some("ab2-only"), None).unwrap();
        assert_eq!(bodies(&one), vec![0]);
        assert!(!one.has_more_before && !one.has_more_after && one.next_before.is_none() && one.next_after.is_none());
        let one = load_window(&conn, "ab2", None, None, Some("ab2-only"), Some(1)).unwrap();
        assert_eq!(one.messages.len(), 1);

        let err = load_window(&conn, "ab2", None, None, Some("ab1-000"), None).err().unwrap();
        assert!(err.contains("not found in this thread"));
        assert!(load_window(&conn, "ab1", None, None, Some("nope"), None).is_err());
    }

    #[test]
    fn after_cursor_and_equal_timestamp_tiebreak_by_id() {
        let conn = fts_conn();
        seed_thread(&conn, "tb1", "e-tb1", None, false);
        for i in 0..7 {
            put(&conn, &format!("tb1-id{i}"), "tb1", Some(&format!("m{i}")), "2020-01-01T00:00:00.000Z");
        }
        // around an id in the middle of equal timestamps
        let p = load_window(&conn, "tb1", None, None, Some("tb1-id3"), Some(3)).unwrap();
        assert_eq!(bodies(&p), vec![2, 3, 4]);
        assert!(p.has_more_before && p.has_more_after);
        // forward walk with `after` from the oldest: stable, no dupes, ends cleanly
        let mut seen = Vec::new();
        let mut page = load_window(&conn, "tb1", None, None, Some("tb1-id0"), Some(1)).unwrap();
        seen.extend(bodies(&page));
        while page.has_more_after {
            page = load_window(&conn, "tb1", None, page.next_after.as_deref(), None, Some(2)).unwrap();
            assert!(page.has_more_before);
            seen.extend(bodies(&page));
        }
        assert_eq!(seen, (0..7).collect::<Vec<_>>());
        assert!(page.next_after.is_none());
        // after the newest row: empty, no more
        let e = load_window(&conn, "tb1", None, Some("2020-01-01T00:00:00.000Z|tb1-id6"), None, None).unwrap();
        assert!(e.messages.is_empty() && !e.has_more_after);
        assert!(load_window(&conn, "tb1", None, Some("garbage"), None, None).is_err());
    }

    #[test]
    fn newest_and_before_pages_report_new_fields() {
        let conn = window_conn("np1", 10);
        let newest = load_window(&conn, "np1", None, None, None, Some(4)).unwrap();
        assert_eq!(bodies(&newest), vec![6, 7, 8, 9]);
        assert!(newest.has_more && newest.has_more_before && !newest.has_more_after && newest.next_after.is_none());
        let older = load_window(&conn, "np1", newest.next_before.as_deref(), None, None, Some(4)).unwrap();
        assert_eq!(bodies(&older), vec![2, 3, 4, 5]);
        assert!(older.has_more_after && older.next_after.is_some());
        // the older page's next_after leads back to the newest page
        let back = load_window(&conn, "np1", None, older.next_after.as_deref(), None, Some(10)).unwrap();
        assert_eq!(bodies(&back), vec![6, 7, 8, 9]);
        // JSON shape is stable and old payloads (no new fields) still deserialize
        let v = serde_json::to_value(&newest).unwrap();
        for k in ["messages", "has_more", "next_before", "has_more_before", "has_more_after", "next_after"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        let old: ChatMessagesPage = serde_json::from_str(r#"{"messages":[],"has_more":false,"next_before":null}"#).unwrap();
        assert!(!old.has_more_after && old.next_after.is_none());
    }

    #[test]
    fn bench_fts_100k() {
        let path = std::env::temp_dir().join(format!("snapvault-fts-bench-{}.db", uuid::Uuid::new_v4()));
        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch(crate::db::PRAGMAS).unwrap();
        crate::db::migrate(&conn, &crate::db::MIGRATIONS[..3], std::path::Path::new("/none")).unwrap();
        conn.execute("INSERT INTO chat_threads (id, external_id, title) VALUES ('bf1', 'e-bf1', 'T')", []).unwrap();
        let words = ["hello", "pizza", "tonight", "lol", "meeting", "tomorrow", "snap", "photo", "friend", "weekend", "cat", "dog", "movie", "dinner", "work", "home"];
        let tx = conn.transaction().unwrap();
        for i in 0..100_000usize {
            let body: Vec<&str> = (0..8).map(|j| words[(i * 7 + j * 13 + i / 5) % words.len()]).collect();
            put(&tx, &format!("bf1-{i}"), "bf1", Some(&body.join(" ")), &format!("2020-01-01T{:02}:{:02}:{:02}.000Z", i / 3600 % 24, i / 60 % 60, i % 60));
        }
        tx.commit().unwrap();
        // v3 db with 100k rows -> v4 backfill.
        let start = std::time::Instant::now();
        crate::db::migrate(&conn, crate::db::MIGRATIONS, std::path::Path::new("/none")).unwrap();
        println!("BENCH migrate v4 backfill 100k: {:?}", start.elapsed());
        for q in ["pizza", "pizza tonight", "piz", "zzzz"] {
            let start = std::time::Instant::now();
            let p = search_messages(&conn, q, None, None, None).unwrap();
            println!("BENCH search {q:?} 100k: {:?} (total {}, capped {})", start.elapsed(), p.total, p.total_capped);
        }
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }
}

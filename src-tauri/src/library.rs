use std::collections::BTreeSet;

use rusqlite::{Connection, ToSql};
use serde::Serialize;

use crate::db::DbState;

const MAX_PAGE_SIZE: i64 = 200;

#[derive(Clone, Serialize)]
pub struct MemoryAsset {
    pub id: String,
    pub media_type: String,
    pub original_path: String,
    pub overlay_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub playback_path: Option<String>,
    pub taken_at: Option<String>,
    pub is_favorite: bool,
    pub tags: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct MemoriesPage {
    pub assets: Vec<MemoryAsset>,
    pub total: i64,
    pub offset: i64,
}

/// Paginates the `memory` assets for the Memories grid. Offset pagination is
/// sufficient here: this is a local, single-writer SQLite file with
/// supporting indexes, not a concurrently-written remote table where offsets
/// could drift.
///
/// `sort` is `"asc"` or `"desc"` (defaults to `"desc"`); `filter` is one of
/// `"all"`, `"photo"`, `"video"`, `"favorite"` (defaults to `"all"`); `tag`
/// optionally restricts to assets carrying that exact tag.
#[tauri::command]
pub fn list_memory_assets(
    state: tauri::State<DbState>,
    offset: i64,
    limit: i64,
    sort: String,
    filter: String,
    tag: Option<String>,
) -> Result<MemoriesPage, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    let offset = offset.max(0);
    let limit = limit.clamp(1, MAX_PAGE_SIZE);
    let tag = tag.filter(|t| !t.trim().is_empty());

    let total = count_matching(&conn, &filter, tag.as_deref())?;
    let assets = load_page(&conn, offset, limit, &sort, &filter, tag.as_deref())?;

    Ok(MemoriesPage {
        assets,
        total,
        offset,
    })
}

/// Distinct tags across every memory asset, for the filter dropdown and the
/// tag editor's suggestions.
#[tauri::command]
pub fn list_memory_tags(state: tauri::State<DbState>) -> Result<Vec<String>, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    let mut stmt = conn
        .prepare("SELECT tags FROM assets WHERE source_type = 'memory' AND tags != '[]'")
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| err.to_string())?;

    let mut tags = BTreeSet::new();
    for row in rows {
        let raw = row.map_err(|err| err.to_string())?;
        if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&raw) {
            tags.extend(parsed);
        }
    }
    Ok(tags.into_iter().collect())
}

#[tauri::command]
pub fn set_asset_favorite(
    state: tauri::State<DbState>,
    asset_id: String,
    is_favorite: bool,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    conn.execute(
        "UPDATE assets SET is_favorite = ?1 WHERE id = ?2",
        rusqlite::params![is_favorite as i64, asset_id],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn set_asset_tags(
    state: tauri::State<DbState>,
    asset_id: String,
    tags: Vec<String>,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    let mut cleaned = Vec::new();
    for tag in tags {
        let tag = tag.trim().to_string();
        if !tag.is_empty() && seen.insert(tag.clone()) {
            cleaned.push(tag);
        }
    }
    let json = serde_json::to_string(&cleaned).map_err(|err| err.to_string())?;

    let conn = state.0.lock().map_err(|err| err.to_string())?;
    conn.execute(
        "UPDATE assets SET tags = ?1 WHERE id = ?2",
        rusqlite::params![json, asset_id],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn media_filter_clause(filter: &str) -> &'static str {
    match filter {
        "photo" => "AND media_type = 'image'",
        "video" => "AND media_type = 'video'",
        "favorite" => "AND is_favorite = 1",
        _ => "",
    }
}

const TAG_FILTER_CLAUSE: &str = "AND EXISTS (SELECT 1 FROM json_each(tags) WHERE value = ?1)";

fn count_matching(conn: &Connection, filter: &str, tag: Option<&str>) -> Result<i64, String> {
    let sql = format!(
        "SELECT COUNT(*) FROM assets WHERE source_type = 'memory' {} {}",
        media_filter_clause(filter),
        if tag.is_some() { TAG_FILTER_CLAUSE } else { "" },
    );
    let result = if let Some(tag) = tag {
        conn.query_row(&sql, [tag], |row| row.get(0))
    } else {
        conn.query_row(&sql, [], |row| row.get(0))
    };
    result.map_err(|err| err.to_string())
}

fn load_page(
    conn: &Connection,
    offset: i64,
    limit: i64,
    sort: &str,
    filter: &str,
    tag: Option<&str>,
) -> Result<Vec<MemoryAsset>, String> {
    let order = if sort == "asc" { "ASC" } else { "DESC" };
    let tag_clause = if tag.is_some() { TAG_FILTER_CLAUSE } else { "" };
    let sql = format!(
        "SELECT id, media_type, original_path, overlay_path, thumbnail_path, playback_path, taken_at, is_favorite, tags
         FROM assets
         WHERE source_type = 'memory' {media} {tag_clause}
         ORDER BY taken_at {order}, id {order}
         LIMIT ?{limit_pos} OFFSET ?{offset_pos}",
        media = media_filter_clause(filter),
        tag_clause = tag_clause,
        order = order,
        limit_pos = if tag.is_some() { 2 } else { 1 },
        offset_pos = if tag.is_some() { 3 } else { 2 },
    );

    let mut stmt = conn.prepare(&sql).map_err(|err| err.to_string())?;

    let mut params: Vec<&dyn ToSql> = Vec::new();
    if let Some(tag) = &tag {
        params.push(tag);
    }
    params.push(&limit);
    params.push(&offset);

    let rows = stmt
        .query_map(rusqlite::params_from_iter(params), |row| {
            let tags_json: String = row.get(8)?;
            Ok(MemoryAsset {
                id: row.get(0)?,
                media_type: row.get(1)?,
                original_path: row.get(2)?,
                overlay_path: row.get(3)?,
                thumbnail_path: row.get(4)?,
                playback_path: row.get(5)?,
                taken_at: row.get(6)?,
                is_favorite: row.get::<_, i64>(7)? != 0,
                tags: serde_json::from_str(&tags_json).unwrap_or_default(),
            })
        })
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;

    fn seed(conn: &Connection, id: &str, taken_at: &str, source_type: &str) {
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, taken_at, source_type)
             VALUES (?1, 'image', ?2, ?3, ?4)",
            rusqlite::params![id, format!("/{id}.jpg"), taken_at, source_type],
        )
        .unwrap();
    }

    #[test]
    fn paginates_memory_assets_newest_first_and_excludes_other_sources() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();

        seed(&conn, "a", "2020-01-01T00:00:00.000Z", "memory");
        seed(&conn, "b", "2020-01-03T00:00:00.000Z", "memory");
        seed(&conn, "c", "2020-01-02T00:00:00.000Z", "memory");
        seed(&conn, "chat-asset", "2020-01-04T00:00:00.000Z", "chat");

        let page1 = load_page(&conn, 0, 2, "desc", "all", None).unwrap();
        assert_eq!(page1.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["b", "c"]);

        let page2 = load_page(&conn, 2, 2, "desc", "all", None).unwrap();
        assert_eq!(page2.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["a"]);
    }

    #[test]
    fn sorts_ascending_when_requested() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed(&conn, "a", "2020-01-01T00:00:00.000Z", "memory");
        seed(&conn, "b", "2020-01-03T00:00:00.000Z", "memory");

        let page = load_page(&conn, 0, 10, "asc", "all", None).unwrap();
        assert_eq!(page.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
    }

    #[test]
    fn filters_by_media_type_and_favorite() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, taken_at, source_type, is_favorite)
             VALUES ('img', 'image', '/img.jpg', '2020-01-01T00:00:00.000Z', 'memory', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, taken_at, source_type, is_favorite)
             VALUES ('vid', 'video', '/vid.mp4', '2020-01-02T00:00:00.000Z', 'memory', 1)",
            [],
        )
        .unwrap();

        let photos = load_page(&conn, 0, 10, "desc", "photo", None).unwrap();
        assert_eq!(photos.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["img"]);

        let favorites = load_page(&conn, 0, 10, "desc", "favorite", None).unwrap();
        assert_eq!(favorites.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["vid"]);

        assert_eq!(count_matching(&conn, "video", None).unwrap(), 1);
        assert_eq!(count_matching(&conn, "all", None).unwrap(), 2);
    }

    #[test]
    fn filters_by_tag() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, taken_at, source_type, tags)
             VALUES ('a', 'image', '/a.jpg', '2020-01-01T00:00:00.000Z', 'memory', '[\"trip\",\"friends\"]')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, taken_at, source_type, tags)
             VALUES ('b', 'image', '/b.jpg', '2020-01-02T00:00:00.000Z', 'memory', '[\"friends\"]')",
            [],
        )
        .unwrap();

        let trip_only = load_page(&conn, 0, 10, "desc", "all", Some("trip")).unwrap();
        assert_eq!(trip_only.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["a"]);
        assert_eq!(count_matching(&conn, "all", Some("friends")).unwrap(), 2);
    }

    #[test]
    fn set_asset_tags_dedupes_trims_and_drops_blanks() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed(&conn, "a", "2020-01-01T00:00:00.000Z", "memory");

        let mut seen = BTreeSet::new();
        let mut cleaned = Vec::new();
        for tag in vec![" trip ".to_string(), "trip".to_string(), "".to_string(), "friends".to_string()] {
            let tag = tag.trim().to_string();
            if !tag.is_empty() && seen.insert(tag.clone()) {
                cleaned.push(tag);
            }
        }
        assert_eq!(cleaned, vec!["trip".to_string(), "friends".to_string()]);
    }

    #[test]
    fn list_memory_tags_collects_distinct_sorted_tags() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, source_type, tags)
             VALUES ('a', 'image', '/a.jpg', 'memory', '[\"trip\",\"friends\"]')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, source_type, tags)
             VALUES ('b', 'image', '/b.jpg', 'memory', '[\"friends\",\"family\"]')",
            [],
        )
        .unwrap();

        let mut stmt = conn
            .prepare("SELECT tags FROM assets WHERE source_type = 'memory' AND tags != '[]'")
            .unwrap();
        let rows = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();
        let mut tags = BTreeSet::new();
        for row in rows {
            let raw = row.unwrap();
            if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&raw) {
                tags.extend(parsed);
            }
        }
        assert_eq!(
            tags.into_iter().collect::<Vec<_>>(),
            vec!["family".to_string(), "friends".to_string(), "trip".to_string()]
        );
    }
}

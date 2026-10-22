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
/// optionally restricts to assets carrying that exact tag; `year` optionally
/// restricts to that calendar year.
#[tauri::command]
pub fn list_memory_assets(
    app: tauri::AppHandle,
    state: tauri::State<DbState>,
    offset: i64,
    limit: i64,
    sort: String,
    filter: String,
    tag: Option<String>,
    year: Option<i32>,
) -> Result<MemoriesPage, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    let offset = offset.max(0);
    let limit = limit.clamp(1, MAX_PAGE_SIZE);
    let tag = tag.filter(|t| !t.trim().is_empty());

    let total = count_matching(&conn, &filter, tag.as_deref(), year)?;
    let mut assets = load_page(&conn, offset, limit, &sort, &filter, tag.as_deref(), year)?;
    // DB paths are media-root-relative; the frontend gets absolute ones.
    let root = crate::storage::resolve_media_root(&app)?;
    let abs = |p: &str| crate::storage::to_absolute(&root, p).display().to_string();
    for a in &mut assets {
        a.original_path = abs(&a.original_path);
        a.overlay_path = a.overlay_path.as_deref().map(abs);
        a.thumbnail_path = a.thumbnail_path.as_deref().map(abs);
        a.playback_path = a.playback_path.as_deref().map(abs);
    }

    Ok(MemoriesPage {
        assets,
        total,
        offset,
    })
}

/// Distinct years (ascending) that have at least one matching memory, for
/// the Memories timeline scrubber's tick labels and the date filter's option
/// list. `year_filter` mirrors whatever date filter is currently active, so
/// the scrubber correctly collapses to a single tick when one is set.
#[tauri::command]
pub fn list_memory_years(
    state: tauri::State<DbState>,
    filter: String,
    tag: Option<String>,
    year_filter: Option<i32>,
) -> Result<Vec<i32>, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    let tag = tag.filter(|t| !t.trim().is_empty());
    list_years(&conn, &filter, tag.as_deref(), year_filter)
}

/// Row offset of the first matching asset at or after `year` (in the given
/// sort order), so the frontend's timeline scrubber can jump the grid
/// straight to that point via `list_memory_assets`. `year_filter` must match
/// whatever date filter `list_memory_assets` is currently called with, or
/// the computed offset won't line up with that filtered result set.
#[tauri::command]
pub fn memory_year_offset(
    state: tauri::State<DbState>,
    year: i32,
    sort: String,
    filter: String,
    tag: Option<String>,
    year_filter: Option<i32>,
) -> Result<i64, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    let tag = tag.filter(|t| !t.trim().is_empty());
    year_offset(&conn, year, &sort, &filter, tag.as_deref(), year_filter)
}

fn list_years(conn: &Connection, filter: &str, tag: Option<&str>, year_filter: Option<i32>) -> Result<Vec<i32>, String> {
    let sql = format!(
        "SELECT DISTINCT CAST(substr(taken_at, 1, 4) AS INTEGER) AS y
         FROM assets
         WHERE source_type = 'memory' AND taken_at IS NOT NULL {media} {tag_clause} {year_clause}
         ORDER BY y ASC",
        media = media_filter_clause(filter),
        tag_clause = if tag.is_some() { TAG_FILTER_CLAUSE } else { "" },
        year_clause = if year_filter.is_some() { YEAR_FILTER_CLAUSE } else { "" },
    );
    let year_str = year_filter.map(|y| format!("{y:04}"));
    let mut stmt = conn.prepare(&sql).map_err(|err| err.to_string())?;
    let mut params: Vec<(&str, &dyn ToSql)> = Vec::new();
    if let Some(tag) = tag.as_ref() {
        params.push((":tag", tag));
    }
    if let Some(year_str) = &year_str {
        params.push((":year", year_str));
    }
    let years = stmt
        .query_map(&params[..], |row| row.get::<_, i32>(0))
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    Ok(years)
}

fn year_offset(
    conn: &Connection,
    year: i32,
    sort: &str,
    filter: &str,
    tag: Option<&str>,
    year_filter: Option<i32>,
) -> Result<i64, String> {
    // DESC lists newest-first, so everything "before" year Y's items is
    // anything from Y+1 onward; ASC lists oldest-first, so it's anything
    // strictly before Y's start.
    let (comparison, bound) = if sort == "asc" {
        ("<", format!("{year:04}-01-01T00:00:00.000Z"))
    } else {
        (">=", format!("{:04}-01-01T00:00:00.000Z", year + 1))
    };
    let sql = format!(
        "SELECT COUNT(*) FROM assets
         WHERE source_type = 'memory' AND taken_at IS NOT NULL {media} {tag_clause} {year_clause}
         AND taken_at {comparison} :bound",
        media = media_filter_clause(filter),
        tag_clause = if tag.is_some() { TAG_FILTER_CLAUSE } else { "" },
        year_clause = if year_filter.is_some() { YEAR_FILTER_CLAUSE } else { "" },
    );
    let year_str = year_filter.map(|y| format!("{y:04}"));
    let mut params: Vec<(&str, &dyn ToSql)> = Vec::new();
    if let Some(tag) = tag.as_ref() {
        params.push((":tag", tag));
    }
    if let Some(year_str) = &year_str {
        params.push((":year", year_str));
    }
    params.push((":bound", &bound));
    conn.query_row(&sql, &params[..], |row| row.get(0)).map_err(|err| err.to_string())
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

/// Single source of truth for which memory rows a (filter, tag, year) selects;
/// used by the grid (count + page) and by export so they cannot diverge.
pub(crate) fn memory_where(filter: &str, tag: Option<&str>, year: Option<i32>) -> String {
    format!(
        "source_type = 'memory' {} {} {}",
        media_filter_clause(filter),
        if tag.is_some() { TAG_FILTER_CLAUSE } else { "" },
        if year.is_some() { YEAR_FILTER_CLAUSE } else { "" },
    )
}

/// Bound values for `memory_where`'s named placeholders.
pub(crate) struct FilterParams {
    tag: Option<String>,
    year: Option<String>,
}

impl FilterParams {
    pub(crate) fn new(tag: Option<&str>, year: Option<i32>) -> Self {
        Self { tag: tag.map(str::to_string), year: year.map(|y| format!("{y:04}")) }
    }

    pub(crate) fn named(&self) -> Vec<(&str, &dyn ToSql)> {
        let mut params: Vec<(&str, &dyn ToSql)> = Vec::new();
        if let Some(tag) = &self.tag {
            params.push((":tag", tag));
        }
        if let Some(year) = &self.year {
            params.push((":year", year));
        }
        params
    }
}

const TAG_FILTER_CLAUSE: &str = "AND EXISTS (SELECT 1 FROM json_each(tags) WHERE value = :tag)";
const YEAR_FILTER_CLAUSE: &str = 
    "AND taken_at >= :year AND taken_at < CAST(CAST(:year AS INTEGER) + 1 AS TEXT)"; // sargable; ISO text sorts by year prefix

fn count_matching(conn: &Connection, filter: &str, tag: Option<&str>, year: Option<i32>) -> Result<i64, String> {
    let sql = format!("SELECT COUNT(*) FROM assets WHERE {}", memory_where(filter, tag, year));
    let fp = FilterParams::new(tag, year);
    conn.query_row(&sql, &fp.named()[..], |row| row.get(0)).map_err(|err| err.to_string())
}

fn load_page(
    conn: &Connection,
    offset: i64,
    limit: i64,
    sort: &str,
    filter: &str,
    tag: Option<&str>,
    year: Option<i32>,
) -> Result<Vec<MemoryAsset>, String> {
    let order = if sort == "asc" { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT id, media_type, original_path, overlay_path, thumbnail_path, playback_path, taken_at, is_favorite, tags
         FROM assets
         WHERE {where_clause}
         ORDER BY taken_at {order}, id {order}
         LIMIT :limit OFFSET :offset",
        where_clause = memory_where(filter, tag, year),
        order = order,
    );

    let mut stmt = conn.prepare(&sql).map_err(|err| err.to_string())?;

    let fp = FilterParams::new(tag, year);
    let mut params = fp.named();
    params.push((":limit", &limit));
    params.push((":offset", &offset));

    let rows = stmt
        .query_map(&params[..], |row| {
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

/// Test helper: ids the grid would list (asc, same order as export).
#[cfg(test)]
pub(crate) fn list_for_test(conn: &Connection, filter: &str, tag: Option<&str>, year: Option<i32>) -> Vec<String> {
    load_page(conn, 0, i64::MAX, "asc", filter, tag, year).unwrap().into_iter().map(|a| a.id).collect()
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

        let page1 = load_page(&conn, 0, 2, "desc", "all", None, None).unwrap();
        assert_eq!(page1.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["b", "c"]);

        let page2 = load_page(&conn, 2, 2, "desc", "all", None, None).unwrap();
        assert_eq!(page2.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["a"]);
    }

    #[test]
    fn sorts_ascending_when_requested() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed(&conn, "a", "2020-01-01T00:00:00.000Z", "memory");
        seed(&conn, "b", "2020-01-03T00:00:00.000Z", "memory");

        let page = load_page(&conn, 0, 10, "asc", "all", None, None).unwrap();
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

        let photos = load_page(&conn, 0, 10, "desc", "photo", None, None).unwrap();
        assert_eq!(photos.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["img"]);

        let favorites = load_page(&conn, 0, 10, "desc", "favorite", None, None).unwrap();
        assert_eq!(favorites.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["vid"]);

        assert_eq!(count_matching(&conn, "video", None, None).unwrap(), 1);
        assert_eq!(count_matching(&conn, "all", None, None).unwrap(), 2);
    }

    #[test]
    fn filters_by_year() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed(&conn, "a", "2019-06-01T00:00:00.000Z", "memory");
        seed(&conn, "b", "2020-06-01T00:00:00.000Z", "memory");
        seed(&conn, "c", "2020-12-01T00:00:00.000Z", "memory");

        let year_2020 = load_page(&conn, 0, 10, "desc", "all", None, Some(2020)).unwrap();
        assert_eq!(year_2020.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["c", "b"]);
        assert_eq!(count_matching(&conn, "all", None, Some(2020)).unwrap(), 2);
        assert_eq!(count_matching(&conn, "all", None, Some(2019)).unwrap(), 1);
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

        let trip_only = load_page(&conn, 0, 10, "desc", "all", Some("trip"), None).unwrap();
        assert_eq!(trip_only.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["a"]);
        assert_eq!(count_matching(&conn, "all", Some("friends"), None).unwrap(), 2);
    }

    #[test]
    fn set_asset_tags_dedupes_trims_and_drops_blanks() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed(&conn, "a", "2020-01-01T00:00:00.000Z", "memory");

        let mut seen = BTreeSet::new();
        let mut cleaned = Vec::new();
        for tag in [" trip ".to_string(), "trip".to_string(), "".to_string(), "friends".to_string()] {
            let tag = tag.trim().to_string();
            if !tag.is_empty() && seen.insert(tag.clone()) {
                cleaned.push(tag);
            }
        }
        assert_eq!(cleaned, vec!["trip".to_string(), "friends".to_string()]);
    }

    #[test]
    fn list_memory_years_returns_distinct_years_ascending_and_ignores_other_sources() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed(&conn, "a", "2021-06-01T00:00:00.000Z", "memory");
        seed(&conn, "b", "2019-01-01T00:00:00.000Z", "memory");
        seed(&conn, "c", "2021-12-01T00:00:00.000Z", "memory");
        seed(&conn, "chat-asset", "2024-01-01T00:00:00.000Z", "chat");

        let years = list_years(&conn, "all", None, None).unwrap();
        assert_eq!(years, vec![2019, 2021]);
    }

    #[test]
    fn list_years_collapses_to_the_active_year_filter() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed(&conn, "a", "2021-06-01T00:00:00.000Z", "memory");
        seed(&conn, "b", "2019-01-01T00:00:00.000Z", "memory");

        let years = list_years(&conn, "all", None, Some(2019)).unwrap();
        assert_eq!(years, vec![2019]);
    }

    #[test]
    fn memory_year_offset_counts_rows_that_sort_before_the_target_year() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed(&conn, "a", "2019-06-01T00:00:00.000Z", "memory");
        seed(&conn, "b", "2020-06-01T00:00:00.000Z", "memory");
        seed(&conn, "c", "2021-06-01T00:00:00.000Z", "memory");

        // DESC (newest first): the 2021 item comes before 2020's.
        assert_eq!(year_offset(&conn, 2020, "desc", "all", None, None).unwrap(), 1);
        // ASC (oldest first): the 2019 item comes before 2020's.
        assert_eq!(year_offset(&conn, 2020, "asc", "all", None, None).unwrap(), 1);
    }

    #[test]
    fn memory_year_offset_respects_the_active_year_filter() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        seed(&conn, "a", "2020-06-01T00:00:00.000Z", "memory");
        seed(&conn, "b", "2020-09-01T00:00:00.000Z", "memory");
        seed(&conn, "c", "2021-06-01T00:00:00.000Z", "memory");

        // Filtered to 2020 only, both 2020 rows sort before/at their own
        // year's target, and the (excluded) 2021 row shouldn't count.
        assert_eq!(year_offset(&conn, 2020, "desc", "all", None, Some(2020)).unwrap(), 0);
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
    #[test]
    fn bench_year_filter_50k() {
        use std::time::Instant;
        let dir = std::env::temp_dir().join(format!("sv_bench_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let mut conn = Connection::open(&dir).unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        let tx = conn.transaction().unwrap();
        for i in 0..50_000u32 {
            let y = 2014 + i % 10;
            let ts = format!("{y:04}-{:02}-{:02}T{:02}:{:02}:00.000Z", 1 + i % 12, 1 + i % 28, i % 24, (i / 24) % 60);
            tx.execute(
                "INSERT INTO assets (id, media_type, original_path, taken_at, source_type, tags, is_favorite)
                 VALUES (?1, ?2, ?3, ?4, 'memory', ?5, ?6)",
                rusqlite::params![format!("id{i:06}"), if i % 3 == 0 { "video" } else { "image" }, format!("/{i}"), ts,
                    if i % 5 == 0 { "[\"trip\"]" } else { "[]" }, (i % 7 == 0) as i64],
            ).unwrap();
        }
        tx.commit().unwrap();
        conn.execute_batch("ANALYZE").unwrap();
        for (label, f, y) in [("all", "all", 2016), ("photo", "photo", 2020), ("fav", "favorite", 2022)] {
            let plan: Vec<String> = conn.prepare(&format!("EXPLAIN QUERY PLAN SELECT id FROM assets WHERE source_type='memory' {} {} ORDER BY taken_at DESC, id DESC LIMIT 60", media_filter_clause(f), YEAR_FILTER_CLAUSE))
                .unwrap().query_map(rusqlite::named_params! {":year": format!("{y:04}")}, |r| r.get::<_, String>(3)).unwrap().map(|r| r.unwrap()).collect();
            let t = Instant::now();
            let mut ids = Vec::new();
            let mut n = 0;
            for _ in 0..20 {
                n = count_matching(&conn, f, None, Some(y)).unwrap();
                ids = load_page(&conn, 0, 60, "desc", f, None, Some(y)).unwrap().into_iter().map(|a| a.id).collect();
            }
            let tt = Instant::now();
            for _ in 0..20 { list_years(&conn, f, None, Some(y)).unwrap(); }
            println!("BENCH {label} y={y}: count+page {:?}/iter, years {:?}/iter, n={n}, first={:?} last={:?} plan={plan:?}", t.elapsed() / 20 - tt.elapsed() / 20 * 0, tt.elapsed() / 20, ids.first(), ids.last());
        }
        let t = Instant::now();
        for _ in 0..20 { let _ = list_years(&conn, "all", None, None).unwrap(); }
        println!("BENCH years-unfiltered {:?}/iter", t.elapsed() / 20);
        let _ = std::fs::remove_file(&dir);
    }
}

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

const MEDIA_SUFFIXES: &[&str] = &[
    "-main", "_main", "-overlay", "_overlay", "-image", "_image", "-video", "_video", "-media",
    "_media", "-caption", "_caption",
];
const OVERLAY_SUFFIXES: &[&str] = &["-overlay", "_overlay", "-caption", "_caption"];

#[derive(Clone, Serialize)]
pub struct ParseMemoriesSummary {
    pub json_items: usize,
    pub files_found: usize,
    pub matched: usize,
    pub unmatched_files: usize,
    pub assets_inserted: usize,
    pub memory_items_inserted: usize,
    pub files_timestamp_repaired: usize,
}

struct MemoryGroup {
    stem: String,
    day_key: String,
    main_path: PathBuf,
    overlay_path: Option<PathBuf>,
    mtime: SystemTime,
}

struct JsonMemoryItem {
    day_key: String,
    taken_at: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    raw: Value,
}

/// Reads `memories_history.json` and the extracted `memories/` files under
/// `job_dir/part-*/`, matches each JSON entry (the authoritative source for
/// timestamp and location, per project rules) to its extracted file,
/// populates `assets`/`memory_items`, and repairs the matched files' OS
/// timestamps. `emit(processed, total, message)` reports progress through
/// this phase for the frontend; pass `&|_, _, _| {}` to run silently.
pub(super) fn parse_memories_blocking(
    conn: &Connection,
    job_dir: &Path,
    emit: &dyn Fn(usize, usize, String),
) -> Result<ParseMemoriesSummary, String> {
    emit(0, 0, "Parsing metadata".to_string());

    let part_dirs = super::find_part_dirs(job_dir)?;

    let json_items = load_saved_media(&part_dirs)?;
    let mut json_by_day: HashMap<String, Vec<JsonMemoryItem>> = HashMap::new();
    for item in json_items {
        json_by_day.entry(item.day_key.clone()).or_default().push(item);
    }
    for items in json_by_day.values_mut() {
        items.sort_by(|a, b| a.taken_at.cmp(&b.taken_at));
    }
    let total_json_items: usize = json_by_day.values().map(|v| v.len()).sum();

    let groups = find_memory_groups(&part_dirs)?;
    let mut groups_by_day: HashMap<String, Vec<MemoryGroup>> = HashMap::new();
    for group in groups {
        groups_by_day.entry(group.day_key.clone()).or_default().push(group);
    }
    for groups in groups_by_day.values_mut() {
        groups.sort_by_key(|g| g.mtime);
    }
    let total_files: usize = groups_by_day.values().map(|v| v.len()).sum();

    emit(
        0,
        total_files,
        format!("Matching {total_json_items} JSON entries against {total_files} files"),
    );
    // Cap event volume the same way extraction does: at most ~100 updates
    // regardless of how many memories there are.
    let emit_step = (total_files / 100).max(1);

    let collection_id = ensure_memory_collection(conn)?;

    let mut matched = 0usize;
    let mut unmatched_files = 0usize;
    let mut assets_inserted = 0usize;
    let mut memory_items_inserted = 0usize;
    let mut files_timestamp_repaired = 0usize;
    let mut position = 0i64;
    let mut processed_files = 0usize;

    for (day_key, groups) in groups_by_day {
        let json_for_day = json_by_day.get(&day_key);
        for (index, group) in groups.into_iter().enumerate() {
            let item = json_for_day.and_then(|items| items.get(index));
            if item.is_some() {
                matched += 1;
            } else {
                unmatched_files += 1;
            }

            let (asset_id, inserted) = upsert_asset(conn, &group, item)?;
            if inserted {
                assets_inserted += 1;
            }

            let taken_at = item
                .and_then(|i| i.taken_at.clone())
                .unwrap_or_else(|| format!("{}T00:00:00.000Z", group.day_key));
            let raw_payload = item.map(|i| i.raw.to_string());

            // JSON is the absolute truth for timestamps (per project rules):
            // once a file is matched to a JSON entry, stamp the *extracted
            // file itself* with that exact time, not just the DB row, so the
            // file's OS-level timestamp matches what Snapchat recorded.
            if item.is_some() {
                let mut paths = vec![group.main_path.as_path()];
                if let Some(overlay) = group.overlay_path.as_deref() {
                    paths.push(overlay);
                }
                files_timestamp_repaired += repair_file_timestamps(&paths, &taken_at);
            }

            let item_inserted = conn
                .execute(
                    "INSERT OR IGNORE INTO memory_items (id, collection_id, asset_id, taken_at, position, raw_payload)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        Uuid::new_v4().to_string(),
                        collection_id,
                        asset_id,
                        taken_at,
                        position,
                        raw_payload,
                    ],
                )
                .map_err(|err| format!("failed to insert memory_item: {err}"))?;
            memory_items_inserted += item_inserted;
            position += 1;

            processed_files += 1;
            if processed_files % emit_step == 0 || processed_files == total_files {
                emit(
                    processed_files,
                    total_files,
                    format!("Repairing timestamps {processed_files}/{total_files}"),
                );
            }
        }
    }

    Ok(ParseMemoriesSummary {
        json_items: total_json_items,
        files_found: total_files,
        matched,
        unmatched_files,
        assets_inserted,
        memory_items_inserted,
        files_timestamp_repaired,
    })
}

fn load_saved_media(part_dirs: &[PathBuf]) -> Result<Vec<JsonMemoryItem>, String> {
    let mut items = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for part_dir in part_dirs {
        let json_path = part_dir.join("json").join("memories_history.json");
        if !json_path.is_file() {
            continue;
        }
        let contents = fs::read_to_string(&json_path)
            .map_err(|err| format!("failed to read {}: {err}", json_path.display()))?;
        let payload: Value = serde_json::from_str(&contents)
            .map_err(|err| format!("failed to parse {}: {err}", json_path.display()))?;
        let saved_media = payload
            .get("Saved Media")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Dedup only guards against the same memories_history.json being
        // repeated verbatim in more than one export part - it must NOT drop
        // repeats within a single part's own list, since Snapchat genuinely
        // emits multiple distinct saved-media entries with identical field
        // values (e.g. a burst of saves at the same second/location).
        let mut seen_this_part = std::collections::HashSet::new();

        for raw in saved_media {
            let signature = raw.to_string();
            if seen.contains(&signature) {
                continue;
            }
            seen_this_part.insert(signature);

            let date_raw = raw.get("Date").and_then(|v| v.as_str());
            let Some((day_key, taken_at)) = date_raw.and_then(parse_snap_date) else {
                continue;
            };
            let (latitude, longitude) = raw
                .get("Location")
                .and_then(|v| v.as_str())
                .and_then(parse_snap_location)
                .map(|(lat, lng)| (Some(lat), Some(lng)))
                .unwrap_or((None, None));

            items.push(JsonMemoryItem {
                day_key,
                taken_at: Some(taken_at),
                latitude,
                longitude,
                raw,
            });
        }

        seen.extend(seen_this_part);
    }

    Ok(items)
}

/// Stamps `paths` (the extracted media file and its overlay, if any) with
/// `taken_at` as both the OS modified time and, on Windows, the creation
/// time - recreating what the old repair_memory_timestamps_from_archives.py
/// script did, except sourced from the JSON `Date` field (the project's
/// declared source of truth) rather than the zip archive's own metadata.
/// Returns how many of `paths` were successfully repaired.
fn repair_file_timestamps(paths: &[&Path], taken_at: &str) -> usize {
    let Some(time) = parse_iso_to_system_time(taken_at) else {
        return 0;
    };
    paths
        .iter()
        .filter(|path| set_file_times(path, time).is_ok())
        .count()
}

fn set_file_times(path: &Path, time: SystemTime) -> std::io::Result<()> {
    let file = fs::OpenOptions::new().write(true).open(path)?;
    let mut times = fs::FileTimes::new().set_modified(time);
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTimesExt;
        times = times.set_created(time);
    }
    file.set_times(times)
}

/// Inverse of the `YYYY-MM-DDTHH:MM:SS.000Z` format `parse_snap_date`
/// produces. Fixed-width fields again mean plain slicing suffices.
fn parse_iso_to_system_time(iso: &str) -> Option<SystemTime> {
    if iso.len() != 24 {
        return None;
    }
    let year: i64 = iso.get(0..4)?.parse().ok()?;
    let month: u32 = iso.get(5..7)?.parse().ok()?;
    let day: u32 = iso.get(8..10)?.parse().ok()?;
    let hour: i64 = iso.get(11..13)?.parse().ok()?;
    let minute: i64 = iso.get(14..16)?.parse().ok()?;
    let second: i64 = iso.get(17..19)?.parse().ok()?;
    let days = super::days_from_civil(year, month, day);
    let secs = days * 86_400 + hour * 3_600 + minute * 60 + second;
    let unix_secs = u64::try_from(secs).ok()?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(unix_secs))
}

/// Snapchat formats memory dates as `"2026-07-04 14:33:51 UTC"`. Fixed-width
/// zero-padded fields mean no calendar arithmetic is needed - just slice and
/// reformat to the app's `YYYY-MM-DDTHH:MM:SS.000Z` convention.
pub(super) fn parse_snap_date(raw: &str) -> Option<(String, String)> {
    let cleaned = raw.trim().trim_end_matches(" UTC").trim();
    let bytes = cleaned.as_bytes();
    if cleaned.len() != 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b' '
        || bytes[13] != b':'
        || bytes[16] != b':'
        || !cleaned[..10].chars().all(|c| c.is_ascii_digit() || c == '-')
    {
        return None;
    }
    let day_key = cleaned[0..10].to_string();
    let taken_at = format!("{}T{}.000Z", &cleaned[0..10], &cleaned[11..19]);
    Some((day_key, taken_at))
}

/// Snapchat formats memory location as `"Latitude, Longitude: 51.73, 5.13"`.
fn parse_snap_location(raw: &str) -> Option<(f64, f64)> {
    let rest = raw.strip_prefix("Latitude, Longitude: ")?;
    let mut parts = rest.split(", ");
    let lat = parts.next()?.trim().parse::<f64>().ok()?;
    let lng = parts.next()?.trim().parse::<f64>().ok()?;
    Some((lat, lng))
}

fn normalize_stem(stem: &str) -> &str {
    for suffix in MEDIA_SUFFIXES {
        if let Some(stripped) = stem.strip_suffix(suffix) {
            return stripped;
        }
    }
    stem
}

fn is_overlay_variant(stem: &str) -> bool {
    OVERLAY_SUFFIXES.iter().any(|suffix| stem.ends_with(suffix))
}

fn filename_date_prefix(stem: &str) -> Option<String> {
    if stem.len() >= 10 && stem.as_bytes()[4] == b'-' && stem.as_bytes()[7] == b'-' {
        Some(stem[0..10].to_string())
    } else {
        None
    }
}

fn find_memory_groups(part_dirs: &[PathBuf]) -> Result<Vec<MemoryGroup>, String> {
    struct Pending {
        main_path: Option<PathBuf>,
        overlay_path: Option<PathBuf>,
        mtime: Option<SystemTime>,
        day_key: Option<String>,
    }

    let mut pending: HashMap<String, Pending> = HashMap::new();

    for part_dir in part_dirs {
        let memories_dir = part_dir.join("memories");
        if !memories_dir.is_dir() {
            continue;
        }
        let entries = fs::read_dir(&memories_dir)
            .map_err(|err| format!("failed to read {}: {err}", memories_dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|err| err.to_string())?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(day_key) = filename_date_prefix(stem) else {
                continue;
            };
            let normalized = normalize_stem(stem).to_string();
            let slot = pending.entry(normalized).or_insert(Pending {
                main_path: None,
                overlay_path: None,
                mtime: None,
                day_key: None,
            });
            slot.day_key.get_or_insert(day_key);

            if is_overlay_variant(stem) {
                slot.overlay_path = Some(path);
            } else if slot.main_path.is_none() {
                let mtime = fs::metadata(&path).and_then(|m| m.modified()).ok();
                slot.main_path = Some(path);
                slot.mtime = mtime;
            }
        }
    }

    let groups = pending
        .into_iter()
        .filter_map(|(stem, slot)| {
            let main_path = slot.main_path?;
            let day_key = slot.day_key?;
            Some(MemoryGroup {
                stem,
                day_key,
                main_path,
                overlay_path: slot.overlay_path,
                mtime: slot.mtime.unwrap_or(SystemTime::UNIX_EPOCH),
            })
        })
        .collect();

    Ok(groups)
}

fn media_type_for_extension(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "mp4" | "mov" | "avi" | "webm" | "m4v" => "video",
        "m4a" | "mp3" | "wav" | "aac" => "audio",
        _ => "image",
    }
}

fn ensure_memory_collection(conn: &Connection) -> Result<String, String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM memory_collections WHERE title = 'Saved Media' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO memory_collections (id, title) VALUES (?1, 'Saved Media')",
        [&id],
    )
    .map_err(|err| format!("failed to create memory collection: {err}"))?;
    Ok(id)
}

fn upsert_asset(
    conn: &Connection,
    group: &MemoryGroup,
    item: Option<&JsonMemoryItem>,
) -> Result<(String, bool), String> {
    let original_path = group.main_path.display().to_string();
    if let Ok(id) = conn.query_row(
        "SELECT id FROM assets WHERE original_path = ?1",
        [&original_path],
        |row| row.get::<_, String>(0),
    ) {
        return Ok((id, false));
    }

    let id = Uuid::new_v4().to_string();
    let external_id = format!("memories:{}", group.stem);
    let extension = group
        .main_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let media_type = media_type_for_extension(extension);
    let file_size_bytes = fs::metadata(&group.main_path).map(|m| m.len()).ok();
    let overlay_path = group
        .overlay_path
        .as_ref()
        .map(|p| p.display().to_string());
    let taken_at = item
        .and_then(|i| i.taken_at.clone())
        .unwrap_or_else(|| format!("{}T00:00:00.000Z", group.day_key));
    let latitude = item.and_then(|i| i.latitude);
    let longitude = item.and_then(|i| i.longitude);
    let raw_metadata = serde_json::json!({ "relative_path": group.stem }).to_string();

    conn.execute(
        "INSERT INTO assets (
            id, external_id, source_type, media_type, original_path, overlay_path,
            file_size_bytes, taken_at, latitude, longitude, raw_metadata
        ) VALUES (?1, ?2, 'memory', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            id,
            external_id,
            media_type,
            original_path,
            overlay_path,
            file_size_bytes,
            taken_at,
            latitude,
            longitude,
            raw_metadata,
        ],
    )
    .map_err(|err| format!("failed to insert asset: {err}"))?;

    Ok((id, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;

    #[test]
    fn parses_snap_date_format() {
        let (day_key, taken_at) = parse_snap_date("2026-07-04 14:33:51 UTC").unwrap();
        assert_eq!(day_key, "2026-07-04");
        assert_eq!(taken_at, "2026-07-04T14:33:51.000Z");
    }

    #[test]
    fn rejects_malformed_snap_date() {
        assert!(parse_snap_date("not a date").is_none());
        assert!(parse_snap_date("").is_none());
    }

    #[test]
    fn parses_snap_location_format() {
        let (lat, lng) = parse_snap_location("Latitude, Longitude: 51.734997, 5.1375704").unwrap();
        assert!((lat - 51.734997).abs() < 1e-9);
        assert!((lng - 5.1375704).abs() < 1e-9);
    }

    #[test]
    fn rejects_empty_snap_location() {
        assert!(parse_snap_location("").is_none());
    }

    #[test]
    fn normalizes_main_and_overlay_to_same_stem() {
        assert_eq!(normalize_stem("2020-12-19_abc-main"), "2020-12-19_abc");
        assert_eq!(normalize_stem("2020-12-19_abc-overlay"), "2020-12-19_abc");
        assert!(is_overlay_variant("2020-12-19_abc-overlay"));
        assert!(!is_overlay_variant("2020-12-19_abc-main"));
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("snapvault-memtest-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn preserves_intra_part_duplicates_but_drops_cross_part_duplicates() {
        // Real Snapchat exports repeat identical entries (same Date/Location)
        // within a single memories_history.json for genuinely distinct saved
        // memories (e.g. a burst saved at the same second). Only a duplicate
        // repeated across *different* export parts should be dropped.
        let job_dir = tempdir();
        let repeated_entry = serde_json::json!({
            "Date": "2020-12-19 14:26:21 UTC",
            "Media Type": "Video",
            "Location": "Latitude, Longitude: 51.591583, 5.3116527",
        });

        for (part_name, entries) in [
            ("part-000", serde_json::json!([repeated_entry, repeated_entry])),
            ("part-001", serde_json::json!([repeated_entry])),
        ] {
            let json_dir = job_dir.join(part_name).join("json");
            fs::create_dir_all(&json_dir).unwrap();
            let payload = serde_json::json!({ "Saved Media": entries });
            fs::write(
                json_dir.join("memories_history.json"),
                serde_json::to_string(&payload).unwrap(),
            )
            .unwrap();
        }

        let part_dirs = super::super::find_part_dirs(&job_dir).unwrap();
        let items = load_saved_media(&part_dirs).unwrap();

        // 2 genuine duplicates within part-000 survive; part-001's copy is a
        // cross-part repeat of an entry already seen and is dropped.
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn parses_memories_json_and_populates_db() {
        let job_dir = tempdir();
        let part_dir = job_dir.join("part-000");
        let memories_dir = part_dir.join("memories");
        let json_dir = part_dir.join("json");
        fs::create_dir_all(&memories_dir).unwrap();
        fs::create_dir_all(&json_dir).unwrap();

        fs::write(
            memories_dir.join("2020-12-19_abc-main.jpg"),
            b"fake-jpg-bytes",
        )
        .unwrap();
        fs::write(
            memories_dir.join("2020-12-19_abc-overlay.png"),
            b"fake-overlay-bytes",
        )
        .unwrap();
        fs::write(
            memories_dir.join("2020-12-19_def-main.mp4"),
            b"fake-mp4-bytes",
        )
        .unwrap();

        // Give "abc" an earlier mtime than "def" so day-bucket ordering is
        // deterministic instead of relying on filesystem enumeration order.
        let earlier = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_608_000_000);
        let later = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_608_003_600);
        fs::OpenOptions::new()
            .write(true)
            .open(memories_dir.join("2020-12-19_abc-main.jpg"))
            .unwrap()
            .set_modified(earlier)
            .unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(memories_dir.join("2020-12-19_def-main.mp4"))
            .unwrap()
            .set_modified(later)
            .unwrap();

        let payload = serde_json::json!({
            "Saved Media": [
                {
                    "Date": "2020-12-19 14:00:00 UTC",
                    "Media Type": "Image",
                    "Location": "Latitude, Longitude: 51.5, 5.3",
                    "Download Link": ""
                },
                {
                    "Date": "2020-12-19 15:00:00 UTC",
                    "Media Type": "Video",
                    "Location": "Latitude, Longitude: 51.6, 5.4",
                    "Download Link": ""
                }
            ]
        });
        fs::write(
            json_dir.join("memories_history.json"),
            serde_json::to_string(&payload).unwrap(),
        )
        .unwrap();

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();

        let summary = parse_memories_blocking(&conn, &job_dir, &|_, _, _| {}).unwrap();
        assert_eq!(summary.json_items, 2);
        assert_eq!(summary.files_found, 2);
        assert_eq!(summary.matched, 2);
        assert_eq!(summary.unmatched_files, 0);
        assert_eq!(summary.assets_inserted, 2);
        assert_eq!(summary.memory_items_inserted, 2);

        let asset_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(asset_count, 2);

        let (taken_at, latitude, overlay_path): (String, f64, Option<String>) = conn
            .query_row(
                "SELECT taken_at, latitude, overlay_path FROM assets WHERE original_path LIKE '%abc-main.jpg'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(taken_at, "2020-12-19T14:00:00.000Z");
        assert!((latitude - 51.5).abs() < 1e-9);
        assert!(overlay_path.unwrap().ends_with("abc-overlay.png"));

        // The repair step must have stamped the actual OS file mtime (not
        // just the DB row) with the JSON-derived timestamp - both the main
        // file and its overlay, replacing the placeholder mtime set above.
        // "abc" contributes 2 paths (main + overlay), "def" contributes 1
        // (no overlay) = 3 files repaired.
        assert_eq!(summary.files_timestamp_repaired, 3);
        let expected_abc = SystemTime::UNIX_EPOCH + Duration::from_secs(1_608_386_400); // 2020-12-19T14:00:00Z
        let expected_def = SystemTime::UNIX_EPOCH + Duration::from_secs(1_608_390_000); // 2020-12-19T15:00:00Z
        let abc_mtime = fs::metadata(memories_dir.join("2020-12-19_abc-main.jpg"))
            .unwrap()
            .modified()
            .unwrap();
        let abc_overlay_mtime = fs::metadata(memories_dir.join("2020-12-19_abc-overlay.png"))
            .unwrap()
            .modified()
            .unwrap();
        let def_mtime = fs::metadata(memories_dir.join("2020-12-19_def-main.mp4"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(abc_mtime, expected_abc);
        assert_eq!(abc_overlay_mtime, expected_abc);
        assert_eq!(def_mtime, expected_def);

        // Re-running on the same job must not duplicate assets or memory_items.
        let second_summary = parse_memories_blocking(&conn, &job_dir, &|_, _, _| {}).unwrap();
        assert_eq!(second_summary.assets_inserted, 0);
        assert_eq!(second_summary.memory_items_inserted, 0);
        let asset_count_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(asset_count_after, 2);
    }

}

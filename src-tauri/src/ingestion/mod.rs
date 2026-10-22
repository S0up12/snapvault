pub mod chats;
pub mod memories;
pub mod profile;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;
use zip::ZipArchive;

use crate::db::DbState;
use crate::thumbnails::{acquire_media_processing_slot, process_pending_media};

const PROGRESS_EVENT: &str = "ingestion://progress";

/// Streamed to the frontend over the `ingestion://progress` event so the UI
/// can drive a single progress bar across every phase of a job (extraction,
/// then JSON parsing + timestamp repair) without polling.
#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ProgressPayload {
    Progress {
        job_id: String,
        phase: String,
        message: String,
        processed: usize,
        total: usize,
        percent: f64,
    },
    Completed {
        job_id: String,
        summary: IngestionSummary,
    },
    Error {
        job_id: String,
        phase: String,
        message: String,
    },
}

fn emit_progress(
    app: &AppHandle,
    job_id: &str,
    phase: &str,
    processed: usize,
    total: usize,
    message: String,
) {
    let percent = if total == 0 {
        100.0
    } else {
        (processed as f64 / total as f64) * 100.0
    };
    let _ = app.emit(
        PROGRESS_EVENT,
        ProgressPayload::Progress {
            job_id: job_id.to_string(),
            phase: phase.to_string(),
            message,
            processed,
            total,
            percent,
        },
    );
}

#[derive(Clone, Serialize)]
pub struct ExtractionSummary {
    pub job_id: String,
    pub destination: String,
    pub extracted_entries: usize,
    pub skipped_entries: usize,
}

#[derive(Clone, Serialize)]
pub struct IngestionSummary {
    pub job_id: String,
    pub destination: String,
    pub extracted_entries: usize,
    pub skipped_entries: usize,
    pub json_items: usize,
    pub files_found: usize,
    pub matched: usize,
    pub unmatched_files: usize,
    pub assets_inserted: usize,
    pub memory_items_inserted: usize,
    pub files_timestamp_repaired: usize,
    pub chat_threads_inserted: usize,
    pub chat_messages_inserted: usize,
    pub chat_media_assets_linked: usize,
    pub profile_found: bool,
    pub thumbnails_generated: usize,
    pub thumbnails_failed: usize,
    pub playback_transcoded: usize,
    pub playback_failed: usize,
    /// Media phase was cancelled; remaining media stays pending.
    #[serde(default)]
    pub media_cancelled: bool,
    /// Extracted files nothing references (e.g. a fully deduped re-import) deleted from the job dir.
    #[serde(default)]
    pub pruned_files: usize,
    #[serde(default)]
    pub pruned_bytes: u64,
}

static INGESTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// RAII "an import is running" flag, held from the start of `run_ingestion`
/// (before extraction) until it returns, errors or panics. Acquisition order:
/// IngestionGuard first, then MediaSlot (media phase). `reset_library` /
/// `set_media_root` check `is_active()` before taking the MediaSlot.
pub(crate) struct IngestionGuard<'a>(&'a std::sync::atomic::AtomicBool);

impl<'a> IngestionGuard<'a> {
    fn acquire_in(flag: &'a std::sync::atomic::AtomicBool) -> Result<Self, String> {
        use std::sync::atomic::Ordering::SeqCst;
        flag.compare_exchange(false, true, SeqCst, SeqCst)
            .map(|_| Self(flag))
            .map_err(|_| "An import is already running.".to_string())
    }
}

impl Drop for IngestionGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

pub(crate) fn is_active() -> bool {
    INGESTING.load(std::sync::atomic::Ordering::SeqCst)
}

/// Err when an import is active (pure over the flag, for tests).
fn ensure_no_import_in(flag: &std::sync::atomic::AtomicBool) -> Result<(), String> {
    if flag.load(std::sync::atomic::Ordering::SeqCst) {
        Err("An import is running; try again when it finishes.".to_string())
    } else {
        Ok(())
    }
}

pub(crate) fn ensure_no_import() -> Result<(), String> {
    ensure_no_import_in(&INGESTING)
}

/// Media root for a job dir, which is always `<root>/imports/<job>`.
pub(super) fn media_root_of(job_dir: &Path) -> PathBuf {
    job_dir.parent().and_then(Path::parent).unwrap_or(job_dir).to_path_buf()
}

/// Runs the whole Phase 3 pipeline for one import job: extracts one or more
/// Snapchat export .zip parts into `<media_root>/imports/<job_id>/part-NNN/`
/// (`media_root` resolved via `storage::resolve_media_root` - the OS app-data
/// dir by default, or a user-chosen folder; large exports are split by
/// Snapchat into several zip parts that each carry a different slice of the
/// account), then parses `memories_history.json`
/// against the extracted files and repairs their OS timestamps. Runs on a
/// background thread via `spawn_blocking` so the UI never stalls, and emits
/// `ingestion://progress` events throughout both phases for the frontend to
/// render as a single progress bar.
#[tauri::command]
pub async fn run_ingestion(
    app: AppHandle,
    archive_paths: Vec<String>,
) -> Result<IngestionSummary, String> {
    let guard = IngestionGuard::acquire_in(&INGESTING)?;
    if archive_paths.is_empty() {
        return Err("no archive files provided".to_string());
    }

    let job_id = Uuid::new_v4().to_string();

    let mut sources = Vec::with_capacity(archive_paths.len());
    for archive_path in &archive_paths {
        let source = PathBuf::from(archive_path);
        if !source.is_file() {
            return Err(format!("archive not found: {}", source.display()));
        }
        let is_zip = source
            .extension()
            .map(|ext| ext.eq_ignore_ascii_case("zip"))
            .unwrap_or(false);
        if !is_zip {
            return Err(format!("expected a .zip file, got {}", source.display()));
        }
        sources.push(source);
    }

    let media_root = crate::storage::resolve_media_root(&app)?;
    let destination = media_root.join("imports").join(&job_id);

    let app_for_blocking = app.clone();
    let job_id_for_blocking = job_id.clone();
    let destination_for_blocking = destination.clone();
    let media_root_for_blocking = media_root.clone();

    let result = tauri::async_runtime::spawn_blocking(
        move || -> Result<IngestionSummary, String> {
            let _guard = guard; // released when the blocking task ends (even on panic)
            let emit_extract = |processed: usize, total: usize, message: String| {
                emit_progress(
                    &app_for_blocking,
                    &job_id_for_blocking,
                    "extracting",
                    processed,
                    total,
                    message,
                );
            };
            let extraction = extract_archives_blocking(
                &job_id_for_blocking,
                &sources,
                &destination_for_blocking,
                &emit_extract,
            )?;

            let state = app_for_blocking.state::<DbState>();
            let conn = state.0.lock().map_err(|err| err.to_string())?;

            let emit_parse = |processed: usize, total: usize, message: String| {
                emit_progress(
                    &app_for_blocking,
                    &job_id_for_blocking,
                    "parsing",
                    processed,
                    total,
                    message,
                );
            };
            let parsing = memories::parse_memories_blocking(
                &conn,
                &destination_for_blocking,
                &emit_parse,
            )?;

            let emit_chats = |processed: usize, total: usize, message: String| {
                emit_progress(
                    &app_for_blocking,
                    &job_id_for_blocking,
                    "parsing_chats",
                    processed,
                    total,
                    message,
                );
            };
            let chats_summary =
                chats::parse_chats_blocking(&conn, &destination_for_blocking, &emit_chats)?;

            let emit_profile = |processed: usize, total: usize, message: String| {
                emit_progress(
                    &app_for_blocking,
                    &job_id_for_blocking,
                    "parsing_profile",
                    processed,
                    total,
                    message,
                );
            };
            let profile_summary =
                profile::build_profile_snapshot_blocking(&conn, &destination_for_blocking, &emit_profile)?;

            // DB phase done: drop extracted files no asset row points at.
            let (pruned_files, pruned_bytes) =
                prune_unreferenced(&conn, &media_root_for_blocking, &destination_for_blocking)?;
            crate::log::log_info!(
                "[ingestion:{job_id_for_blocking}] pruned {pruned_files} unreferenced files ({pruned_bytes} bytes)"
            );

            // Release the lock before the (potentially long, one ffmpeg call
            // per asset) media-processing phase so other commands aren't
            // frozen out of the DB for the rest of the job - process_pending_media
            // re-locks briefly on its own between ffmpeg calls instead.
            drop(conn);

            let emit_media = |processed: usize, total: usize, message: String| {
                emit_progress(
                    &app_for_blocking,
                    &job_id_for_blocking,
                    "processing_media",
                    processed,
                    total,
                    message,
                );
            };
            // Waits its turn rather than erroring if the startup auto-resume
            // (or a manual "Reprocess media" click) already holds this slot -
            // both would otherwise pull the same pending assets and race
            // writing the same `{id}.tmp.mp4` output paths.
            let _slot = acquire_media_processing_slot(&app_for_blocking)?;
            let performance_settings = crate::settings::get_performance_settings(app_for_blocking.clone());
            let media_summary = performance_settings.and_then(|settings| {
                process_pending_media(
                    &state.0,
                    &media_root_for_blocking,
                    &settings,
                    &crate::thumbnails::media_ctl(&app_for_blocking),
                    &emit_media,
                )
            });
            let media_summary = media_summary?;

            Ok(IngestionSummary {
                job_id: job_id_for_blocking.clone(),
                destination: extraction.destination,
                extracted_entries: extraction.extracted_entries,
                skipped_entries: extraction.skipped_entries,
                json_items: parsing.json_items,
                files_found: parsing.files_found,
                matched: parsing.matched,
                unmatched_files: parsing.unmatched_files,
                assets_inserted: parsing.assets_inserted,
                memory_items_inserted: parsing.memory_items_inserted,
                files_timestamp_repaired: parsing.files_timestamp_repaired,
                chat_threads_inserted: chats_summary.threads_inserted,
                chat_messages_inserted: chats_summary.messages_inserted,
                chat_media_assets_linked: chats_summary.media_assets_linked,
                profile_found: profile_summary.profile_found,
                thumbnails_generated: media_summary.generated,
                thumbnails_failed: media_summary.failed,
                playback_transcoded: media_summary.playback_transcoded,
                playback_failed: media_summary.playback_failed,
                media_cancelled: media_summary.cancelled,
                pruned_files,
                pruned_bytes,
            })
        },
    )
    .await
    .map_err(|err| format!("ingestion task panicked: {err}"))?;

    match &result {
        Ok(summary) => {
            crate::log::log_info!(
                "[ingestion:{job_id}] completed: extracted={} matched={} unmatched={} assets_inserted={}",
                summary.extracted_entries, summary.matched, summary.unmatched_files, summary.assets_inserted
            );
            let _ = app.emit(
                PROGRESS_EVENT,
                ProgressPayload::Completed {
                    job_id: job_id.clone(),
                    summary: summary.clone(),
                },
            );
        }
        Err(message) => {
            crate::log::log_error!("[ingestion:{job_id}] failed: {message}");
            let _ = app.emit(
                PROGRESS_EVENT,
                ProgressPayload::Error {
                    job_id: job_id.clone(),
                    phase: "ingestion".to_string(),
                    message: message.clone(),
                },
            );
        }
    }

    result
}

/// Case-insensitive on Windows, like the filesystem.
fn path_key(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if cfg!(windows) { s.to_lowercase() } else { s }
}

/// Deletes every file under `job_dir` that no `assets` row references
/// (original/overlay/playback/thumbnail), then empty dirs (the job dir itself
/// included). Chat media links go through asset rows, and no later step reads
/// the extracted JSON/HTML (parsed into the DB), so those are pruned too.
/// Stays inside `job_dir`, never follows symlinks (they are left untouched).
/// Returns (files, bytes) removed.
fn prune_unreferenced(
    conn: &rusqlite::Connection,
    media_root: &Path,
    job_dir: &Path,
) -> Result<(usize, u64), String> {
    let mut keep = std::collections::HashSet::new();
    let mut stmt = conn
        .prepare("SELECT original_path, overlay_path, playback_path, thumbnail_path FROM assets")
        .map_err(|err| format!("failed to list asset paths: {err}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok([r.get::<_, Option<String>>(0)?, r.get(1)?, r.get(2)?, r.get(3)?])
        })
        .map_err(|err| format!("failed to list asset paths: {err}"))?;
    for row in rows {
        for stored in row.map_err(|err| err.to_string())?.into_iter().flatten() {
            keep.insert(path_key(&crate::storage::to_absolute(media_root, &stored)));
        }
    }
    let mut totals = (0usize, 0u64);
    // Refuse anything that is not a real directory (symlinked job dir, missing).
    if fs::symlink_metadata(job_dir).is_ok_and(|m| m.is_dir()) {
        prune_dir(job_dir, &keep, &mut totals);
    }
    Ok(totals)
}

fn prune_dir(dir: &Path, keep: &std::collections::HashSet<String>, totals: &mut (usize, u64)) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        if meta.is_dir() {
            prune_dir(&path, keep, totals);
        } else if meta.is_file() && !keep.contains(&path_key(&path)) && fs::remove_file(&path).is_ok() {
            totals.0 += 1;
            totals.1 += meta.len();
        }
    }
    let _ = fs::remove_dir(dir); // only succeeds when empty
}

/// Finds the `part-NNN` directories every ingestion job's files live under
/// (one per source zip - see `extract_archives_blocking`). Shared by the
/// memories/chats/profile parsers, which all walk the same job layout.
fn find_part_dirs(job_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut parts: Vec<PathBuf> = fs::read_dir(job_dir)
        .map_err(|err| format!("failed to read {}: {err}", job_dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("part-"))
        })
        .collect();
    parts.sort();
    Ok(parts)
}

fn open_zip(source: &Path) -> Result<ZipArchive<io::BufReader<fs::File>>, String> {
    let file = fs::File::open(source)
        .map_err(|err| format!("failed to open archive {}: {err}", source.display()))?;
    ZipArchive::new(io::BufReader::new(file)).map_err(|err| {
        format!(
            "failed to read archive {} (is it a valid zip?): {err}",
            source.display()
        )
    })
}

/// Converts a zip entry's (timezone-less) MS-DOS timestamp to a `SystemTime`
/// by treating its fields as literal UTC, mirroring the old Python ingestion
/// script's `datetime(*member.date_time, tzinfo=UTC)`. It's not actually UTC
/// (zip timestamps are local time to whoever created the archive), but both
/// this extractor and the later JSON-matching step agree on that same
/// convention, which is all that matters for day-bucketing and ordering.
fn zip_datetime_to_system_time(dt: zip::DateTime) -> Option<std::time::SystemTime> {
    let days = days_from_civil(dt.year() as i64, dt.month() as u32, dt.day() as u32);
    let secs = days * 86_400
        + dt.hour() as i64 * 3_600
        + dt.minute() as i64 * 60
        + dt.second() as i64;
    let unix_secs = u64::try_from(secs).ok()?;
    Some(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(unix_secs))
}

/// Howard Hinnant's `days_from_civil`: days since the Unix epoch for a given
/// Gregorian calendar date. Pure arithmetic, no calendar library needed.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ponytail: fixed generous caps; free-disk check skipped (needs a new dep or
// platform calls). Raise if a real export ever hits them.
const MAX_ENTRIES: usize = 500_000;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024 * 1024;

/// Rejects raw zip entry names that are unsafe or non-portable on Windows:
/// backslashes, drive letters, reserved device names, trailing dot/space.
fn is_safe_entry_name(name: &str) -> bool {
    if name.contains('\\') || name.contains('\0') || name.contains(':') {
        return false;
    }
    let b = name.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        return false;
    }
    name.split('/').all(|comp| {
        if comp.ends_with('.') && comp != "." && comp != ".." || comp.ends_with(' ') {
            return false;
        }
        let base = comp.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((base.starts_with("COM") || base.starts_with("LPT"))
                && base.len() == 4
                && base.as_bytes()[3].is_ascii_digit()
                && base.as_bytes()[3] != b'0');
        !reserved
    })
}

/// Extracts into `destination`; on any failure the partial directory is removed.
fn extract_archives_blocking(
    job_id: &str,
    sources: &[PathBuf],
    destination: &Path,
    emit: &dyn Fn(usize, usize, String),
) -> Result<ExtractionSummary, String> {
    let result = extract_archives_inner(job_id, sources, destination, emit);
    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }
    result
}

fn extract_archives_inner(
    job_id: &str,
    sources: &[PathBuf],
    destination: &Path,
    emit: &dyn Fn(usize, usize, String),
) -> Result<ExtractionSummary, String> {
    fs::create_dir_all(destination)
        .map_err(|err| format!("failed to create destination {}: {err}", destination.display()))?;

    // First pass: sum entry counts across every part so progress percentage
    // reflects the whole job, not just whichever part is currently running.
    let mut total_entries = 0usize;
    for source in sources {
        total_entries += open_zip(source)?.len();
    }
    if total_entries > MAX_ENTRIES {
        return Err(format!(
            "archive has {total_entries} entries (limit {MAX_ENTRIES})"
        ));
    }
    let mut bytes_left = MAX_TOTAL_BYTES;

    emit(0, total_entries, format!("Extracting 0/{total_entries} files"));

    // Cap event volume for huge exports: emit at most ~200 progress updates
    // regardless of archive size, so a 20k-file export doesn't flood the IPC
    // bridge and hammer React with re-renders.
    let emit_step = (total_entries / 200).max(1);

    let mut extracted_entries = 0usize;
    let mut skipped_entries = 0usize;
    let mut processed = 0usize;

    for (part_index, source) in sources.iter().enumerate() {
        let part_dir = destination.join(format!("part-{part_index:03}"));
        fs::create_dir_all(&part_dir)
            .map_err(|err| format!("failed to create dir {}: {err}", part_dir.display()))?;

        let mut archive = open_zip(source)?;

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|err| {
                format!(
                    "failed to read entry {index} of {}: {err}",
                    source.display()
                )
            })?;

            // `enclosed_name()` is the zip crate's zip-slip guard: it returns
            // `None` for absolute paths or paths containing `..` components
            // that would otherwise escape the destination directory.
            let symlink = entry
                .unix_mode()
                .is_some_and(|m| m & 0o170000 == 0o120000);
            if symlink || !is_safe_entry_name(entry.name()) {
                skipped_entries += 1;
                processed += 1;
                continue;
            }
            let Some(relative_path) = entry.enclosed_name() else {
                skipped_entries += 1;
                continue;
            };

            let out_path = part_dir.join(&relative_path);
            // Defense in depth beyond enclosed_name(): confirm the resolved
            // path still lives under this part's directory before touching
            // the filesystem.
            if !out_path.starts_with(&part_dir) {
                skipped_entries += 1;
                continue;
            }

            if entry.is_dir() {
                fs::create_dir_all(&out_path).map_err(|err| {
                    format!("failed to create dir {}: {err}", out_path.display())
                })?;
            } else {
                let modified = entry.last_modified().and_then(zip_datetime_to_system_time);
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent).map_err(|err| {
                        format!("failed to create dir {}: {err}", parent.display())
                    })?;
                }
                // create_new: a duplicate entry name must not overwrite the first.
                let mut out_file = match fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&out_path)
                {
                    Ok(f) => f,
                    Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                        skipped_entries += 1;
                        processed += 1;
                        continue;
                    }
                    Err(err) => {
                        return Err(format!(
                            "failed to create file {}: {err}",
                            out_path.display()
                        ))
                    }
                };
                // Cap on real bytes written (declared sizes can lie).
                let written = io::copy(&mut io::Read::take(&mut entry, bytes_left + 1), &mut out_file)
                    .map_err(|err| {
                        format!("failed to write file {}: {err}", out_path.display())
                    })?;
                if written > bytes_left {
                    return Err(format!(
                        "archive exceeds the {} GiB extraction limit",
                        MAX_TOTAL_BYTES >> 30
                    ));
                }
                bytes_left -= written;
                // Snapchat's export zip carries each file's real save time as
                // its zip-entry timestamp; preserving it as the extracted
                // file's mtime is what later lets JSON parsing order same-day
                // memories correctly (filenames alone carry no time-of-day).
                if let Some(modified) = modified {
                    let _ = out_file.set_modified(modified);
                }
                extracted_entries += 1;
            }

            processed += 1;
            if processed.is_multiple_of(emit_step) || processed == total_entries {
                emit(
                    processed,
                    total_entries,
                    format!(
                        "Extracting {processed}/{total_entries} files - {}",
                        relative_path.display()
                    ),
                );
            }
        }
    }

    Ok(ExtractionSummary {
        job_id: job_id.to_string(),
        destination: destination.display().to_string(),
        extracted_entries,
        skipped_entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{Duration, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    #[test]
    fn ingestion_guard_blocks_second_run_and_maintenance() {
        let flag = std::sync::atomic::AtomicBool::new(false);
        assert!(ensure_no_import_in(&flag).is_ok());
        let g = IngestionGuard::acquire_in(&flag).unwrap();
        assert_eq!(IngestionGuard::acquire_in(&flag).err().unwrap(), "An import is already running.");
        assert_eq!(
            ensure_no_import_in(&flag).unwrap_err(),
            "An import is running; try again when it finishes."
        );
        drop(g);
        assert!(ensure_no_import_in(&flag).is_ok());
        assert!(IngestionGuard::acquire_in(&flag).is_ok());
    }

    #[test]
    fn ingestion_guard_released_after_error_and_panic() {
        let flag = std::sync::atomic::AtomicBool::new(false);
        let r: Result<(), String> = (|| {
            let _g = IngestionGuard::acquire_in(&flag)?;
            Err("boom".to_string())
        })();
        assert!(r.is_err());
        assert!(IngestionGuard::acquire_in(&flag).is_ok());
        let p = std::panic::catch_unwind(|| {
            let _g = IngestionGuard::acquire_in(&flag).unwrap();
            panic!("x");
        });
        assert!(p.is_err());
        assert!(IngestionGuard::acquire_in(&flag).is_ok());
    }

    #[test]
    fn days_from_civil_matches_known_epoch_offsets() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 1, 1), 10_957);
    }

    #[test]
    fn zip_datetime_to_system_time_round_trips() {
        // MS-DOS timestamps only have 2-second resolution, so `21` round-trips
        // through `DateTime` as `20` - assert against what the type reports,
        // not the literal we passed in.
        let dt = zip::DateTime::from_date_and_time(2020, 12, 19, 14, 26, 21).unwrap();
        let time = zip_datetime_to_system_time(dt).unwrap();
        let secs = time.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let expected = days_from_civil(2020, 12, 19) as u64 * 86_400
            + dt.hour() as u64 * 3600
            + dt.minute() as u64 * 60
            + dt.second() as u64;
        assert_eq!(secs, expected);
        assert_eq!(time, UNIX_EPOCH + Duration::from_secs(secs));
    }

    fn build_test_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, contents) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn extracts_normal_entries_and_reports_counts() {
        let tmp = tempdir();
        let zip_path = tmp.join("export.zip");
        build_test_zip(
            &zip_path,
            &[
                ("memories_history.json", b"{}"),
                ("media/photo1.jpg", b"fake-jpg-bytes"),
            ],
        );

        let destination = tmp.join("dest");
        let app = &(); // unused, extraction logic below doesn't need a real AppHandle for this check
        let _ = app;

        let file = fs::File::open(&zip_path).unwrap();
        let mut archive = ZipArchive::new(io::BufReader::new(file)).unwrap();
        fs::create_dir_all(&destination).unwrap();

        let mut extracted = 0usize;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let Some(rel) = entry.enclosed_name() else {
                continue;
            };
            let out_path = destination.join(&rel);
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let mut out_file = fs::File::create(&out_path).unwrap();
            io::copy(&mut entry, &mut out_file).unwrap();
            extracted += 1;
        }

        assert_eq!(extracted, 2);
        assert!(destination.join("memories_history.json").is_file());
        assert!(destination.join("media/photo1.jpg").is_file());
    }

    #[test]
    fn extracts_multiple_parts_into_separate_part_dirs() {
        // Mirrors how Snapchat splits large exports into several zips (html
        // in one, memories in another) that must land under one job so a
        // later phase can walk all parts together.
        let tmp = tempdir();
        let part_a = tmp.join("mydata.zip");
        let part_b = tmp.join("mydata-2.zip");
        build_test_zip(&part_a, &[("html/account_history.html", b"<html></html>")]);
        build_test_zip(
            &part_b,
            &[("memories/2020-01-01_abc-main.jpg", b"fake-jpg-bytes")],
        );

        let destination = tmp.join("dest");
        fs::create_dir_all(&destination).unwrap();

        for (part_index, source) in [&part_a, &part_b].into_iter().enumerate() {
            let part_dir = destination.join(format!("part-{part_index:03}"));
            fs::create_dir_all(&part_dir).unwrap();
            let file = fs::File::open(source).unwrap();
            let mut archive = ZipArchive::new(io::BufReader::new(file)).unwrap();
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).unwrap();
                let rel = entry.enclosed_name().unwrap();
                let out_path = part_dir.join(&rel);
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                let mut out_file = fs::File::create(&out_path).unwrap();
                io::copy(&mut entry, &mut out_file).unwrap();
            }
        }

        assert!(destination
            .join("part-000/html/account_history.html")
            .is_file());
        assert!(destination
            .join("part-001/memories/2020-01-01_abc-main.jpg")
            .is_file());
    }

    #[test]
    fn rejects_zip_slip_path_traversal() {
        let tmp = tempdir();
        let zip_path = tmp.join("malicious.zip");
        // A raw zip entry name attempting to escape the destination dir.
        build_test_zip(&zip_path, &[("../../evil.txt", b"pwned")]);

        let file = fs::File::open(&zip_path).unwrap();
        let mut archive = ZipArchive::new(io::BufReader::new(file)).unwrap();
        let entry = archive.by_index(0).unwrap();

        // This is exactly the guard extract_archive_blocking relies on.
        assert!(entry.enclosed_name().is_none());
    }

    #[test]
    fn rejects_unsafe_entry_names() {
        for bad in ["a\\b.jpg", "C:/x.jpg", "c:evil", "dir/CON.txt", "nul", "COM1.png", "x/LPT9", "a.", "a /b"] {
            assert!(!is_safe_entry_name(bad), "{bad}");
        }
        for ok in ["memories/2020-01-01_a-main.jpg", "json/chat_history.json", "COM0", "console.txt"] {
            assert!(is_safe_entry_name(ok), "{ok}");
        }
    }

    #[test]
    fn skips_unsafe_and_duplicate_entries() {
        let tmp = tempdir();
        let zip_path = tmp.join("e.zip");
        build_test_zip(
            &zip_path,
            &[("ok.txt", b"first"), ("C:/evil.txt", b"x"), ("a\\b.txt", b"x")],
        );
        let dest = tmp.join("dest");
        let summary = extract_archives_blocking("j", &[zip_path], &dest, &|_, _, _| {});
        // Duplicate names may be rejected by the zip writer/reader; either way nothing is overwritten.
        if let Ok(s) = summary {
            assert_eq!(s.extracted_entries, 1);
            assert_eq!(fs::read(dest.join("part-000/ok.txt")).unwrap(), b"first");
            assert!(!dest.join("part-000/evil.txt").exists());
        }
    }

    #[test]
    fn failed_extraction_removes_partial_destination() {
        let tmp = tempdir();
        let good = tmp.join("good.zip");
        build_test_zip(&good, &[("a.txt", b"a")]);
        let dest = tmp.join("imports").join("job");
        let missing = tmp.join("missing.zip");
        let res = extract_archives_blocking("j", &[good, missing], &dest, &|_, _, _| {});
        assert!(res.is_err());
        assert!(!dest.exists());
    }

    /// Timing check (run with --nocapture): 5k memories + 20k messages on a
    /// temp-file WAL db. Also asserts a failed phase leaves no partial rows.
    #[test]
    fn bench_parse_insert_phase() {
        let job = tempdir();
        let part = job.join("part-000");
        fs::create_dir_all(part.join("memories")).unwrap();
        fs::create_dir_all(part.join("json")).unwrap();
        let mut saved = Vec::new();
        for i in 0..5000 {
            let (day, sec) = (1 + i / 200, i % 200);
            fs::write(part.join("memories").join(format!("2020-01-{day:02}_m{i}-main.jpg")), b"x").unwrap();
            saved.push(serde_json::json!({"Date": format!("2020-01-{day:02} 10:{:02}:{:02} UTC", sec / 60, sec % 60), "Media Type": "Image", "Location": "Latitude, Longitude: 51.5, 5.3", "n": i}));
        }
        fs::write(part.join("json/memories_history.json"), serde_json::json!({ "Saved Media": saved }).to_string()).unwrap();
        let mut chats = serde_json::Map::new();
        for c in 0..20 {
            let msgs: Vec<_> = (0..1000)
                .map(|m| serde_json::json!({"From": "f", "Media Type": "TEXT", "Created": format!("2020-01-01 {:02}:{:02}:{:02} UTC", m / 3600, m / 60 % 60, m % 60), "Content": "hi", "Created(microseconds)": m, "Media IDs": ""}))
                .collect();
            chats.insert(format!("chat{c}"), msgs.into());
        }
        fs::write(part.join("json/chat_history.json"), serde_json::Value::Object(chats).to_string()).unwrap();

        let conn = rusqlite::Connection::open(job.join("t.db")).unwrap();
        conn.execute_batch(crate::db::PRAGMAS).unwrap();
        conn.execute_batch(crate::db::schema::SCHEMA_SQL).unwrap();
        let t = std::time::Instant::now();
        let m = memories::parse_memories_blocking(&conn, &job, &|_, _, _| {}).unwrap();
        let t_mem = t.elapsed();
        let t = std::time::Instant::now();
        let c = chats::parse_chats_blocking(&conn, &job, &|_, _, _| {}).unwrap();
        println!("BENCH memories={} in {t_mem:?}; chats={} in {:?}", m.memory_items_inserted, c.messages_inserted, t.elapsed());
        assert_eq!(m.assets_inserted, 5000);
        assert_eq!(c.messages_inserted, 20000);
    }

    /// Writes a synthetic job under `<root>/imports/<name>` and ingests it.
    fn ingest_job(conn: &rusqlite::Connection, root: &Path, name: &str, extra: bool) -> PathBuf {
        let job = root.join("imports").join(name);
        let part = job.join("part-000");
        for d in ["memories", "json", "chat_media"] {
            fs::create_dir_all(part.join(d)).unwrap();
        }
        fs::write(part.join("memories/2020-12-19_aaa-main.jpg"), b"jpg").unwrap();
        fs::write(part.join("memories/2020-12-19_aaa-overlay.png"), b"png").unwrap();
        fs::write(part.join("chat_media/2026-07-07_b~Abc.jpg"), b"chat").unwrap();
        let mut saved = vec![serde_json::json!({"Date": "2020-12-19 10:00:00 UTC", "Media Type": "Image"})];
        if extra {
            fs::write(part.join("memories/2021-01-02_bbb-main.jpg"), b"jpg2").unwrap();
            saved.push(serde_json::json!({"Date": "2021-01-02 10:00:00 UTC", "Media Type": "Image"}));
        }
        fs::write(part.join("json/memories_history.json"), serde_json::json!({ "Saved Media": saved }).to_string()).unwrap();
        let chats = serde_json::json!({ "f": [{
            "From": "f", "Media Type": "MEDIA", "Created": "2026-07-07 22:44:03 UTC", "Content": "",
            "Created(microseconds)": 1, "Media IDs": "b~Abc"
        }]});
        fs::write(part.join("json/chat_history.json"), chats.to_string()).unwrap();
        memories::parse_memories_blocking(conn, &job, &|_, _, _| {}).unwrap();
        chats::parse_chats_blocking(conn, &job, &|_, _, _| {}).unwrap();
        job
    }

    fn files_under(dir: &Path) -> usize {
        fs::read_dir(dir).map_or(0, |rd| {
            rd.flatten()
                .map(|e| if e.path().is_dir() { files_under(&e.path()) } else { 1 })
                .sum()
        })
    }

    fn test_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn, crate::db::MIGRATIONS, Path::new("/none")).unwrap();
        conn
    }

    #[test]
    fn prune_keeps_referenced_and_removes_fully_duplicate_job() {
        let root = tempdir();
        let conn = test_db();
        let job1 = ingest_job(&conn, &root, "j1", false);
        // First import: only the json files are unreferenced (3 media referenced).
        let (n, bytes) = prune_unreferenced(&conn, &root, &job1).unwrap();
        assert_eq!(n, 2);
        assert!(bytes > 0);
        assert_eq!(files_under(&job1), 3);
        assert!(job1.join("part-000/memories/2020-12-19_aaa-overlay.png").is_file());
        assert!(!job1.join("part-000/json").exists()); // empty dir removed

        // Re-import of the same export: everything dedupes, whole job dir goes.
        let job2 = ingest_job(&conn, &root, "j2", false);
        let (n, _) = prune_unreferenced(&conn, &root, &job2).unwrap();
        assert_eq!(n, 5);
        assert!(!job2.exists());
        assert_eq!(files_under(&job1), 3);
        let assets: i64 = conn.query_row("SELECT COUNT(*) FROM assets", [], |r| r.get(0)).unwrap();
        assert_eq!(assets, 2);
        // Idempotent / missing dir is a no-op.
        assert_eq!(prune_unreferenced(&conn, &root, &job2).unwrap(), (0, 0));
    }

    #[test]
    fn prune_partial_overlap_keeps_only_new_assets() {
        let root = tempdir();
        let conn = test_db();
        let job1 = ingest_job(&conn, &root, "j1", false);
        prune_unreferenced(&conn, &root, &job1).unwrap();
        let job2 = ingest_job(&conn, &root, "j2", true);
        prune_unreferenced(&conn, &root, &job2).unwrap();
        // Only the new memory survives in job2; duplicates + json are gone.
        assert_eq!(files_under(&job2), 1);
        assert!(job2.join("part-000/memories/2021-01-02_bbb-main.jpg").is_file());
        assert!(!job2.join("part-000/chat_media").exists());
        assert_eq!(files_under(&job1), 3);
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("snapvault-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}

/// Reads the file as bytes once and parses from the slice (no UTF-8 `String`
/// copy). `from_reader` would use less memory but is ~2x slower to parse.
pub(super) fn read_json_file(path: &Path) -> Result<serde_json::Value, String> {
    let bytes = fs::read(path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

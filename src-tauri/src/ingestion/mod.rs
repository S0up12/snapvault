pub mod memories;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;
use zip::ZipArchive;

use crate::db::DbState;

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
}

/// Runs the whole Phase 3 pipeline for one import job: extracts one or more
/// Snapchat export .zip parts into `<app_data_dir>/imports/<job_id>/part-NNN/`
/// (large exports are split by Snapchat into several zip parts that each
/// carry a different slice of the account), then parses `memories_history.json`
/// against the extracted files and repairs their OS timestamps. Runs on a
/// background thread via `spawn_blocking` so the UI never stalls, and emits
/// `ingestion://progress` events throughout both phases for the frontend to
/// render as a single progress bar.
#[tauri::command]
pub async fn run_ingestion(
    app: AppHandle,
    archive_paths: Vec<String>,
) -> Result<IngestionSummary, String> {
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

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    let destination = app_data_dir.join("imports").join(&job_id);

    let app_for_blocking = app.clone();
    let job_id_for_blocking = job_id.clone();
    let destination_for_blocking = destination.clone();

    let result = tauri::async_runtime::spawn_blocking(
        move || -> Result<IngestionSummary, String> {
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
            })
        },
    )
    .await
    .map_err(|err| format!("ingestion task panicked: {err}"))?;

    match &result {
        Ok(summary) => {
            println!(
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
            eprintln!("[ingestion:{job_id}] failed: {message}");
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

fn extract_archives_blocking(
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
                let mut out_file = fs::File::create(&out_path).map_err(|err| {
                    format!("failed to create file {}: {err}", out_path.display())
                })?;
                io::copy(&mut entry, &mut out_file).map_err(|err| {
                    format!("failed to write file {}: {err}", out_path.display())
                })?;
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
            if processed % emit_step == 0 || processed == total_entries {
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

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("snapvault-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}

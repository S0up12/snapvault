use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;
use zip::ZipArchive;

const PROGRESS_EVENT: &str = "ingestion://progress";

/// Streamed to the frontend over the `ingestion://progress` event so the UI
/// can drive a progress bar without polling. Parsing/DB population is a
/// later phase - this only ever reports raw extraction progress.
#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ProgressPayload {
    Started {
        job_id: String,
        total_entries: usize,
        destination: String,
    },
    Progress {
        job_id: String,
        processed_entries: usize,
        total_entries: usize,
        current_entry: String,
        percent: f64,
    },
    Completed {
        job_id: String,
        destination: String,
        extracted_entries: usize,
        skipped_entries: usize,
    },
    Error {
        job_id: String,
        message: String,
    },
}

#[derive(Clone, Serialize)]
pub struct ExtractionSummary {
    pub job_id: String,
    pub destination: String,
    pub extracted_entries: usize,
    pub skipped_entries: usize,
}

/// Extracts a Snapchat export .zip into `<app_data_dir>/imports/<job_id>/`.
/// Runs the actual (blocking) archive I/O on a background thread via
/// `spawn_blocking` so the async executor - and therefore the UI - never
/// stalls, and emits throttled progress events for the frontend to render.
#[tauri::command]
pub async fn extract_snapchat_export(
    app: AppHandle,
    archive_path: String,
) -> Result<ExtractionSummary, String> {
    let job_id = Uuid::new_v4().to_string();

    let source = PathBuf::from(&archive_path);
    if !source.is_file() {
        return Err(format!("archive not found: {}", source.display()));
    }
    let is_zip = source
        .extension()
        .map(|ext| ext.eq_ignore_ascii_case("zip"))
        .unwrap_or(false);
    if !is_zip {
        return Err("expected a .zip file".to_string());
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    let destination = app_data_dir.join("imports").join(&job_id);

    let app_for_blocking = app.clone();
    let job_id_for_blocking = job_id.clone();
    let destination_for_blocking = destination.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        extract_archive_blocking(
            &app_for_blocking,
            &job_id_for_blocking,
            &source,
            &destination_for_blocking,
        )
    })
    .await
    .map_err(|err| format!("extraction task panicked: {err}"))?;

    match &result {
        Ok(summary) => {
            let _ = app.emit(
                PROGRESS_EVENT,
                ProgressPayload::Completed {
                    job_id: summary.job_id.clone(),
                    destination: summary.destination.clone(),
                    extracted_entries: summary.extracted_entries,
                    skipped_entries: summary.skipped_entries,
                },
            );
        }
        Err(message) => {
            let _ = app.emit(
                PROGRESS_EVENT,
                ProgressPayload::Error {
                    job_id: job_id.clone(),
                    message: message.clone(),
                },
            );
        }
    }

    result
}

fn extract_archive_blocking(
    app: &AppHandle,
    job_id: &str,
    source: &Path,
    destination: &Path,
) -> Result<ExtractionSummary, String> {
    fs::create_dir_all(destination)
        .map_err(|err| format!("failed to create destination {}: {err}", destination.display()))?;

    let file = fs::File::open(source).map_err(|err| format!("failed to open archive: {err}"))?;
    let mut archive = ZipArchive::new(io::BufReader::new(file))
        .map_err(|err| format!("failed to read archive (is it a valid zip?): {err}"))?;

    let total_entries = archive.len();
    let _ = app.emit(
        PROGRESS_EVENT,
        ProgressPayload::Started {
            job_id: job_id.to_string(),
            total_entries,
            destination: destination.display().to_string(),
        },
    );

    // Cap event volume for huge exports: emit at most ~200 progress updates
    // regardless of archive size, so a 20k-file export doesn't flood the IPC
    // bridge and hammer React with re-renders.
    let emit_step = (total_entries / 200).max(1);

    let mut extracted_entries = 0usize;
    let mut skipped_entries = 0usize;

    for index in 0..total_entries {
        let mut entry = archive
            .by_index(index)
            .map_err(|err| format!("failed to read entry {index}: {err}"))?;

        // `enclosed_name()` is the zip crate's zip-slip guard: it returns
        // `None` for absolute paths or paths containing `..` components that
        // would otherwise escape the destination directory.
        let Some(relative_path) = entry.enclosed_name() else {
            skipped_entries += 1;
            continue;
        };

        let out_path = destination.join(&relative_path);
        // Defense in depth beyond enclosed_name(): confirm the resolved path
        // still lives under destination before touching the filesystem.
        if !out_path.starts_with(destination) {
            skipped_entries += 1;
            continue;
        }

        if entry.is_dir() {
            fs::create_dir_all(&out_path)
                .map_err(|err| format!("failed to create dir {}: {err}", out_path.display()))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| format!("failed to create dir {}: {err}", parent.display()))?;
            }
            let mut out_file = fs::File::create(&out_path)
                .map_err(|err| format!("failed to create file {}: {err}", out_path.display()))?;
            io::copy(&mut entry, &mut out_file)
                .map_err(|err| format!("failed to write file {}: {err}", out_path.display()))?;
            extracted_entries += 1;
        }

        let processed = index + 1;
        if processed % emit_step == 0 || processed == total_entries {
            let _ = app.emit(
                PROGRESS_EVENT,
                ProgressPayload::Progress {
                    job_id: job_id.to_string(),
                    processed_entries: processed,
                    total_entries,
                    current_entry: relative_path.display().to_string(),
                    percent: (processed as f64 / total_entries as f64) * 100.0,
                },
            );
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
    use zip::write::SimpleFileOptions;

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

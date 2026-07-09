use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::db::DbState;

const PROGRESS_EVENT: &str = "thumbnails://progress";
const THUMBNAIL_SIZE: &str = "360:360";
const WEBP_QUALITY: &str = "72";

#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ProgressPayload {
    Progress {
        processed: usize,
        total: usize,
        percent: f64,
        message: String,
    },
    Completed {
        summary: ThumbnailSummary,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Serialize)]
pub struct ThumbnailSummary {
    pub total: usize,
    pub generated: usize,
    pub failed: usize,
}

struct PendingAsset {
    id: String,
    original_path: String,
    overlay_path: Option<String>,
}

fn emit_progress(app: &AppHandle, processed: usize, total: usize, message: String) {
    let percent = if total == 0 {
        100.0
    } else {
        (processed as f64 / total as f64) * 100.0
    };
    let _ = app.emit(
        PROGRESS_EVENT,
        ProgressPayload::Progress {
            processed,
            total,
            percent,
            message,
        },
    );
}

/// Generates thumbnails for every `image`/`video` asset that doesn't have one
/// yet, streaming `thumbnails://progress` events so the frontend can drive a
/// single progress bar. Runs on a background thread via `spawn_blocking`
/// since it shells out to ffmpeg per asset and waits on each child process.
#[tauri::command]
pub async fn generate_thumbnails(app: AppHandle) -> Result<ThumbnailSummary, String> {
    let app_for_blocking = app.clone();
    let result = tauri::async_runtime::spawn_blocking(
        move || -> Result<ThumbnailSummary, String> {
            let thumbnail_dir = app_for_blocking
                .path()
                .app_data_dir()
                .map_err(|err| format!("failed to resolve app data dir: {err}"))?
                .join("thumbnails");
            std::fs::create_dir_all(&thumbnail_dir).map_err(|err| {
                format!(
                    "failed to create thumbnails dir {}: {err}",
                    thumbnail_dir.display()
                )
            })?;

            let state = app_for_blocking.state::<DbState>();
            let conn = state.0.lock().map_err(|err| err.to_string())?;

            let pending = load_pending_assets(&conn)?;
            let total = pending.len();
            emit_progress(&app_for_blocking, 0, total, "Generating thumbnails".to_string());

            if total == 0 {
                return Ok(ThumbnailSummary {
                    total: 0,
                    generated: 0,
                    failed: 0,
                });
            }

            // Same IPC-flooding guard as the ingestion progress events: cap
            // updates to ~200 regardless of library size.
            let emit_step = (total / 200).max(1);

            let mut generated = 0usize;
            let mut failed = 0usize;
            for (index, asset) in pending.iter().enumerate() {
                let output_path = thumbnail_dir.join(format!("{}.webp", asset.id));
                match generate_one_thumbnail(asset, &output_path) {
                    Ok(()) => {
                        conn.execute(
                            "UPDATE assets SET thumbnail_path = ?1 WHERE id = ?2",
                            rusqlite::params![output_path.display().to_string(), asset.id],
                        )
                        .map_err(|err| {
                            format!("failed to update thumbnail_path for {}: {err}", asset.id)
                        })?;
                        generated += 1;
                    }
                    Err(err) => {
                        eprintln!("[thumbnails] failed for asset {}: {err}", asset.id);
                        failed += 1;
                    }
                }

                let processed = index + 1;
                if processed % emit_step == 0 || processed == total {
                    emit_progress(
                        &app_for_blocking,
                        processed,
                        total,
                        format!("Generating thumbnails {processed}/{total}"),
                    );
                }
            }

            Ok(ThumbnailSummary {
                total,
                generated,
                failed,
            })
        },
    )
    .await
    .map_err(|err| format!("thumbnail generation task panicked: {err}"))?;

    match &result {
        Ok(summary) => {
            let _ = app.emit(
                PROGRESS_EVENT,
                ProgressPayload::Completed {
                    summary: summary.clone(),
                },
            );
        }
        Err(message) => {
            let _ = app.emit(
                PROGRESS_EVENT,
                ProgressPayload::Error {
                    message: message.clone(),
                },
            );
        }
    }

    result
}

fn load_pending_assets(conn: &Connection) -> Result<Vec<PendingAsset>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, original_path, overlay_path FROM assets
             WHERE thumbnail_path IS NULL AND media_type IN ('image', 'video')",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(PendingAsset {
                id: row.get(0)?,
                original_path: row.get(1)?,
                overlay_path: row.get(2)?,
            })
        })
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    Ok(rows)
}

/// Grabs frame 0 (videos) or the source image itself - ffmpeg treats a still
/// image input as a single-frame video stream, so one code path covers both
/// - composites the caption/drawing overlay if present (mirrors the old
/// Python `MediaProcessor`'s PIL `alpha_composite` step), scales to fit
/// 360x360, and encodes as WebP. No `-ss` seek: it's a no-op for video (we
/// already want frame 0) and actively breaks the image2 demuxer for still
/// images, which drops the only frame and produces an empty file.
fn generate_one_thumbnail(asset: &PendingAsset, output_path: &Path) -> Result<(), String> {
    let input = PathBuf::from(&asset.original_path);
    if !input.is_file() {
        return Err(format!("source file missing: {}", input.display()));
    }

    let mut command = Command::new("ffmpeg");
    command.args(["-y", "-i"]).arg(&input);

    let overlay = asset
        .overlay_path
        .as_deref()
        .filter(|path| Path::new(path).is_file());

    if let Some(overlay) = overlay {
        command.arg("-i").arg(overlay);
        command.args([
            "-frames:v",
            "1",
            "-filter_complex",
            &format!(
                "[1:v][0:v]scale2ref=w=iw:h=ih[ovr][base];[base][ovr]overlay=format=auto,scale={THUMBNAIL_SIZE}:force_original_aspect_ratio=decrease:flags=lanczos"
            ),
        ]);
    } else {
        command.args([
            "-frames:v",
            "1",
            "-vf",
            &format!("scale={THUMBNAIL_SIZE}:force_original_aspect_ratio=decrease:flags=lanczos"),
        ]);
    }

    // Force the plain libwebp encoder rather than ffmpeg's default codec
    // pick for a ".webp" output (libwebp_anim, the animated-WebP wrapper),
    // which crashes assembling a single-frame animation for some inputs.
    command
        .args(["-c:v", "libwebp", "-quality", WEBP_QUALITY])
        .arg(output_path);

    let output = command
        .output()
        .map_err(|err| format!("failed to run ffmpeg (is it installed and on PATH?): {err}"))?;

    if !output.status.success() {
        return Err(format!(
            "ffmpeg exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;

    #[test]
    fn load_pending_assets_excludes_audio_and_already_thumbnailed() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();

        conn.execute(
            "INSERT INTO assets (id, media_type, original_path) VALUES ('img', 'image', '/a.jpg')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path) VALUES ('vid', 'video', '/b.mp4')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path) VALUES ('aud', 'audio', '/c.m4a')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, thumbnail_path) VALUES ('done', 'image', '/d.jpg', '/d.webp')",
            [],
        )
        .unwrap();

        let pending = load_pending_assets(&conn).unwrap();
        let ids: Vec<&str> = pending.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"img"));
        assert!(ids.contains(&"vid"));
    }

    #[test]
    fn generate_one_thumbnail_reports_missing_source_file() {
        let asset = PendingAsset {
            id: "missing".to_string(),
            original_path: "/does/not/exist.jpg".to_string(),
            overlay_path: None,
        };
        let err = generate_one_thumbnail(&asset, Path::new("/tmp/out.webp")).unwrap_err();
        assert!(err.contains("source file missing"));
    }
}

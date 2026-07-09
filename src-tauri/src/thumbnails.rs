use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::db::DbState;

const PROGRESS_EVENT: &str = "thumbnails://progress";
const THUMBNAIL_SIZE: &str = "360:360";
const WEBP_QUALITY: &str = "72";
// Codecs WebView2/Chromium can decode without extra OS codec packs. Anything
// else (HEVC/H.265 is the common Snapchat-export case, but also covers .mov
// containers etc.) gets remuxed/transcoded into a playback-safe copy.
const PLAYBACK_SAFE_VIDEO_CODECS: &[&str] = &["h264"];
const PLAYBACK_SAFE_AUDIO_CODECS: &[&str] = &["aac", "mp3"];
const PLAYBACK_SAFE_EXTENSIONS: &[&str] = &["mp4", "m4v"];

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
    pub playback_transcoded: usize,
    pub playback_failed: usize,
}

struct PendingAsset {
    id: String,
    original_path: String,
    overlay_path: Option<String>,
    needs_thumbnail: bool,
    needs_playback: bool,
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
/// yet, and ensures every video has a browser-decodable `playback_path` copy
/// (see `ensure_playback_copy`). Shared by the standalone `generate_thumbnails`
/// command (manual reprocessing/backfill) and `run_ingestion`'s automatic
/// `processing_media` phase, so newly-imported libraries never need the
/// manual step - it only remains for reprocessing older imports or retrying
/// failures. `emit(processed, total, message)` reports progress; pass
/// `&|_, _, _| {}` to run silently.
pub(crate) fn process_pending_media(
    conn: &Connection,
    app_data_dir: &Path,
    emit: &dyn Fn(usize, usize, String),
) -> Result<ThumbnailSummary, String> {
    let thumbnail_dir = app_data_dir.join("thumbnails");
    std::fs::create_dir_all(&thumbnail_dir).map_err(|err| {
        format!(
            "failed to create thumbnails dir {}: {err}",
            thumbnail_dir.display()
        )
    })?;

    let pending = load_pending_assets(conn)?;
    let total = pending.len();
    emit(0, total, "Processing media".to_string());

    if total == 0 {
        return Ok(ThumbnailSummary {
            total: 0,
            generated: 0,
            failed: 0,
            playback_transcoded: 0,
            playback_failed: 0,
        });
    }

    let playback_dir = app_data_dir.join("playback");
    std::fs::create_dir_all(&playback_dir).map_err(|err| {
        format!("failed to create playback dir {}: {err}", playback_dir.display())
    })?;

    // Same IPC-flooding guard as the ingestion progress events: cap updates
    // to ~200 regardless of library size.
    let emit_step = (total / 200).max(1);

    let mut generated = 0usize;
    let mut failed = 0usize;
    let mut playback_transcoded = 0usize;
    let mut playback_failed = 0usize;
    for (index, asset) in pending.iter().enumerate() {
        if asset.needs_thumbnail {
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
        }

        if asset.needs_playback {
            match ensure_playback_copy(asset, &playback_dir) {
                Ok(Some(playback_path)) => {
                    conn.execute(
                        "UPDATE assets SET playback_path = ?1 WHERE id = ?2",
                        rusqlite::params![playback_path.display().to_string(), asset.id],
                    )
                    .map_err(|err| {
                        format!("failed to update playback_path for {}: {err}", asset.id)
                    })?;
                    playback_transcoded += 1;
                }
                Ok(None) => {
                    // Already browser-safe (e.g. plain H.264/AAC mp4) - nothing to store.
                }
                Err(err) => {
                    eprintln!("[thumbnails] playback transcode failed for asset {}: {err}", asset.id);
                    playback_failed += 1;
                }
            }
        }

        let processed = index + 1;
        if processed % emit_step == 0 || processed == total {
            emit(processed, total, format!("Processing media {processed}/{total}"));
        }
    }

    Ok(ThumbnailSummary {
        total,
        generated,
        failed,
        playback_transcoded,
        playback_failed,
    })
}

/// Tauri command wrapping `process_pending_media` for manual reprocessing:
/// backfilling libraries imported before this feature existed, or retrying
/// assets that failed. New imports process automatically via `run_ingestion`.
/// Runs on a background thread via `spawn_blocking` since it shells out to
/// ffmpeg/ffprobe per asset and waits on each child process.
#[tauri::command]
pub async fn generate_thumbnails(app: AppHandle) -> Result<ThumbnailSummary, String> {
    let app_for_blocking = app.clone();
    let result = tauri::async_runtime::spawn_blocking(
        move || -> Result<ThumbnailSummary, String> {
            let app_data_dir = app_for_blocking
                .path()
                .app_data_dir()
                .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
            let state = app_for_blocking.state::<DbState>();
            let conn = state.0.lock().map_err(|err| err.to_string())?;

            let emit = |processed: usize, total: usize, message: String| {
                emit_progress(&app_for_blocking, processed, total, message);
            };
            process_pending_media(&conn, &app_data_dir, &emit)
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
            "SELECT id, original_path, overlay_path,
                    thumbnail_path IS NULL,
                    media_type = 'video' AND playback_path IS NULL
             FROM assets
             WHERE media_type IN ('image', 'video')
               AND (thumbnail_path IS NULL OR (media_type = 'video' AND playback_path IS NULL))",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(PendingAsset {
                id: row.get(0)?,
                original_path: row.get(1)?,
                overlay_path: row.get(2)?,
                needs_thumbnail: row.get(3)?,
                needs_playback: row.get(4)?,
            })
        })
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    Ok(rows)
}

/// Ensures `asset` has a browser-decodable copy of its video, remuxing or
/// transcoding into `playback_dir/{id}.mp4` when the source isn't already
/// H.264/AAC in an mp4 container - mirrors the old Python
/// `MediaProcessor.ensure_browser_playback`. Returns `Ok(None)` when the
/// source is already playback-safe (nothing to store).
fn ensure_playback_copy(asset: &PendingAsset, playback_dir: &Path) -> Result<Option<PathBuf>, String> {
    let input = PathBuf::from(&asset.original_path);
    if !input.is_file() {
        return Err(format!("source file missing: {}", input.display()));
    }

    if !needs_playback_transcode(&input)? {
        return Ok(None);
    }

    let output_path = playback_dir.join(format!("{}.mp4", asset.id));
    let tmp_path = playback_dir.join(format!("{}.mp4.tmp", asset.id));

    let output = Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(&input)
        .args(["-map", "0:v:0", "-map", "0:a:0?"])
        .args(["-c:v", "libx264", "-preset", "medium", "-crf", "23", "-pix_fmt", "yuv420p"])
        .args(["-movflags", "+faststart"])
        .args(["-c:a", "aac", "-b:a", "128k"])
        .arg(&tmp_path)
        .output()
        .map_err(|err| format!("failed to run ffmpeg (is it installed and on PATH?): {err}"))?;

    if !output.status.success() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!(
            "ffmpeg exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    std::fs::rename(&tmp_path, &output_path)
        .map_err(|err| format!("failed to finalize playback copy {}: {err}", output_path.display()))?;

    Ok(Some(output_path))
}

/// Runs `ffprobe` against `path` and returns whether it needs a playback
/// transcode: `true` if the container isn't mp4/m4v, or its video codec
/// isn't H.264, or its audio codec isn't AAC/MP3 - the set WebView2's
/// Chromium engine can decode without extra OS codec packs. This is why
/// Snapchat's HEVC-encoded memory videos play audio with no picture: the
/// audio track (AAC) decodes fine, the video track (HEVC) silently doesn't.
fn needs_playback_transcode(path: &Path) -> Result<bool, String> {
    let extension_ok = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| PLAYBACK_SAFE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false);
    if !extension_ok {
        return Ok(true);
    }

    let output = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_name,codec_type", "-of", "json"])
        .arg(path)
        .output()
        .map_err(|err| format!("failed to run ffprobe (is it installed and on PATH?): {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "ffprobe exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let payload: Value = serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("failed to parse ffprobe output: {err}"))?;
    let streams = payload.get("streams").and_then(|v| v.as_array()).cloned().unwrap_or_default();

    for stream in streams {
        let codec_type = stream.get("codec_type").and_then(|v| v.as_str()).unwrap_or("");
        let codec_name = stream.get("codec_name").and_then(|v| v.as_str()).unwrap_or("");
        let is_safe = match codec_type {
            "video" => PLAYBACK_SAFE_VIDEO_CODECS.contains(&codec_name),
            "audio" => PLAYBACK_SAFE_AUDIO_CODECS.contains(&codec_name),
            _ => true,
        };
        if !is_safe {
            return Ok(true);
        }
    }

    Ok(false)
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
            needs_thumbnail: true,
            needs_playback: false,
        };
        let err = generate_one_thumbnail(&asset, Path::new("/tmp/out.webp")).unwrap_err();
        assert!(err.contains("source file missing"));
    }

    #[test]
    fn needs_playback_transcode_rejects_non_mp4_containers_without_probing() {
        // .mov (and anything outside mp4/m4v) is flagged unsafe purely from
        // the extension, so this doesn't need ffprobe/ffmpeg on PATH to run.
        assert!(needs_playback_transcode(Path::new("/does/not/exist.mov")).unwrap());
    }

    #[test]
    fn load_pending_assets_picks_up_videos_missing_playback_on_existing_installs() {
        // A video thumbnailed before this feature existed has thumbnail_path
        // set but playback_path NULL - it must still be picked up so
        // existing libraries get backfilled, not just newly-imported ones.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, thumbnail_path) VALUES ('vid', 'video', '/b.mp4', '/b.webp')",
            [],
        )
        .unwrap();

        let pending = load_pending_assets(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(!pending[0].needs_thumbnail);
        assert!(pending[0].needs_playback);
    }

    #[test]
    fn ensure_playback_copy_reports_missing_source_file() {
        let asset = PendingAsset {
            id: "missing".to_string(),
            original_path: "/does/not/exist.mp4".to_string(),
            overlay_path: None,
            needs_thumbnail: false,
            needs_playback: true,
        };
        let err = ensure_playback_copy(&asset, Path::new(".")).unwrap_err();
        assert!(err.contains("source file missing"));
    }
}

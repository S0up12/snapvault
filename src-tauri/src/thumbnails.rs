use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

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

/// Tracks whether `process_pending_media` is currently running, so the
/// startup auto-resume and a manual "Reprocess media" click can't overlap -
/// both would otherwise pull the same pending assets and race writing the
/// same `{id}.tmp.mp4` output paths.
#[derive(Default)]
pub struct MediaProcessingState(AtomicBool);

struct PendingAsset {
    id: String,
    media_type: String,
    original_path: String,
    overlay_path: Option<String>,
    /// Non-`None` only for the older side-by-side scheme, where it points at
    /// a separate `playback/{id}.mp4` rather than `original_path` itself -
    /// `ensure_playback_copy` folds those in instead of re-encoding.
    playback_path: Option<String>,
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
/// Takes the DB mutex rather than an already-locked `Connection`: each asset
/// in the loop below needs the DB only for a quick read/write either side of
/// its (slow) ffmpeg call, and re-locking per asset instead of holding one
/// guard for the whole batch means other commands (`list_memory_assets`,
/// `list_chat_threads`, ...) aren't frozen out for the entire run.
pub(crate) fn process_pending_media(
    db: &Mutex<Connection>,
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

    let pending = {
        let conn = db.lock().map_err(|err| err.to_string())?;
        load_pending_assets(&conn)?
    };
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

    // Same IPC-flooding guard as the ingestion progress events: cap updates
    // to ~200 regardless of library size.
    let emit_step = (total / 200).max(1);

    let mut generated = 0usize;
    let mut failed = 0usize;
    let mut playback_transcoded = 0usize;
    let mut playback_failed = 0usize;
    for (index, asset) in pending.iter().enumerate() {
        // Snapchat voice notes are exported as audio-only .mp4 containers,
        // which `media_type_for_extension` classifies as `video` since it
        // only looks at the extension - there's no video stream for ffmpeg
        // to grab a frame from or transcode. Reclassify on sight instead of
        // retrying a doomed thumbnail/transcode every single run. A probe
        // error defaults to "has video" so a flaky ffprobe call doesn't
        // wrongly reclassify a real video.
        if asset.media_type == "video"
            && (asset.needs_thumbnail || asset.needs_playback)
            && !has_video_stream(Path::new(&asset.original_path)).unwrap_or(true)
        {
            let conn = db.lock().map_err(|err| err.to_string())?;
            conn.execute(
                "UPDATE assets SET media_type = 'audio' WHERE id = ?1",
                rusqlite::params![asset.id],
            )
            .map_err(|err| format!("failed to reclassify asset {}: {err}", asset.id))?;
            drop(conn);

            let processed = index + 1;
            if processed % emit_step == 0 || processed == total {
                emit(processed, total, format!("Processing media {processed}/{total}"));
            }
            continue;
        }

        if asset.needs_thumbnail {
            let output_path = thumbnail_dir.join(format!("{}.webp", asset.id));
            match generate_one_thumbnail(asset, &output_path) {
                Ok(()) => {
                    let conn = db.lock().map_err(|err| err.to_string())?;
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
            match ensure_playback_copy(asset) {
                Ok(Some(final_path)) => {
                    let final_path = final_path.display().to_string();
                    let conn = db.lock().map_err(|err| err.to_string())?;
                    let result = if final_path == asset.original_path {
                        conn.execute(
                            "UPDATE assets SET playback_path = ?1 WHERE id = ?2",
                            rusqlite::params![final_path, asset.id],
                        )
                    } else {
                        // Extension had to change (source wasn't .mp4/.m4v) -
                        // original_path now points at a renamed file too.
                        conn.execute(
                            "UPDATE assets SET original_path = ?1, playback_path = ?1 WHERE id = ?2",
                            rusqlite::params![final_path, asset.id],
                        )
                    };
                    result.map_err(|err| format!("failed to update playback_path for {}: {err}", asset.id))?;
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

/// Blocks (briefly polling) until no other `process_pending_media` run holds
/// `MediaProcessingState`, then claims it. `run_ingestion`'s media phase uses
/// this - unlike the manual command/startup resume below, it can't just bail
/// out with "already running" mid-import, so it waits its turn instead.
pub(crate) fn acquire_media_processing_slot(app: &AppHandle) {
    let state = app.state::<MediaProcessingState>();
    while state.0.swap(true, Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

pub(crate) fn release_media_processing_slot(app: &AppHandle) {
    app.state::<MediaProcessingState>().0.store(false, Ordering::SeqCst);
}

/// Shared by the manual `generate_thumbnails` command and the startup
/// auto-resume task. Guards against both running at once (and against either
/// racing `run_ingestion`'s media phase - see `acquire_media_processing_slot`)
/// via `MediaProcessingState`, then runs `process_pending_media` on a
/// background thread (it shells out to ffmpeg/ffprobe per asset and waits on
/// each child process) and emits the same `thumbnails://progress` events
/// either way.
async fn run_and_emit(app: AppHandle) -> Result<ThumbnailSummary, String> {
    if app.state::<MediaProcessingState>().0.swap(true, Ordering::SeqCst) {
        return Err("Media processing is already running.".to_string());
    }

    let app_for_blocking = app.clone();
    let result = tauri::async_runtime::spawn_blocking(
        move || -> Result<ThumbnailSummary, String> {
            let media_root = crate::storage::resolve_media_root(&app_for_blocking)?;
            let state = app_for_blocking.state::<DbState>();

            let emit = |processed: usize, total: usize, message: String| {
                emit_progress(&app_for_blocking, processed, total, message);
            };
            process_pending_media(&state.0, &media_root, &emit)
        },
    )
    .await
    .map_err(|err| format!("thumbnail generation task panicked: {err}"));

    release_media_processing_slot(&app);
    let result = result.and_then(|inner| inner);

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

/// Tauri command wrapping `run_and_emit` for manual reprocessing: backfilling
/// libraries imported before this feature existed, or retrying assets that
/// failed. New imports process automatically via `run_ingestion`, and any
/// backlog left over from an interrupted run resumes automatically at startup
/// (see `resume_pending_media_on_startup`) - this remains for triggering a
/// retry on demand without restarting the app.
#[tauri::command]
pub async fn generate_thumbnails(app: AppHandle) -> Result<ThumbnailSummary, String> {
    run_and_emit(app).await
}

/// Resumes any unfinished media processing left over from a prior run (the
/// app closing or crashing mid-batch, evidenced by orphaned `.tmp.mp4` files
/// and videos stuck without a `playback_path`) without waiting for someone to
/// notice and click "Reprocess media". Fire-and-forget: errors are logged,
/// not surfaced to any caller, since there isn't one at startup.
pub fn resume_pending_media_on_startup(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(err) = run_and_emit(app).await {
            eprintln!("[thumbnails] startup auto-resume failed: {err}");
        }
    });
}

fn load_pending_assets(conn: &Connection) -> Result<Vec<PendingAsset>, String> {
    // `playback_path IS NOT NULL AND playback_path != original_path` catches
    // assets left over from the older side-by-side scheme (a separate
    // `playback/{id}.mp4`, HEVC original untouched) - `needs_playback` covers
    // both that migration case and "never processed at all" (NULL).
    let mut stmt = conn
        .prepare(
            "SELECT id, media_type, original_path, overlay_path, playback_path,
                    thumbnail_path IS NULL,
                    media_type = 'video' AND (playback_path IS NULL OR playback_path != original_path)
             FROM assets
             WHERE media_type IN ('image', 'video')
               AND (thumbnail_path IS NULL
                    OR (media_type = 'video' AND (playback_path IS NULL OR playback_path != original_path)))",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(PendingAsset {
                id: row.get(0)?,
                media_type: row.get(1)?,
                original_path: row.get(2)?,
                overlay_path: row.get(3)?,
                playback_path: row.get(4)?,
                needs_thumbnail: row.get(5)?,
                needs_playback: row.get(6)?,
            })
        })
        .map_err(|err| err.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| err.to_string())?;
    Ok(rows)
}

/// Probes for at least one video stream via ffprobe. Snapchat voice-note
/// chat attachments are audio-only .mp4 containers that `process_pending_media`
/// uses this to detect and reclassify (see its doc comment).
fn has_video_stream(path: &Path) -> Result<bool, String> {
    let output = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v",
            "-show_entries", "stream=codec_type",
            "-of", "json",
        ])
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
    Ok(payload
        .get("streams")
        .and_then(|v| v.as_array())
        .map(|streams| !streams.is_empty())
        .unwrap_or(false))
}

/// Ensures `asset.original_path` is browser-decodable, transcoding H.264/AAC
/// directly over the original file when it isn't - unlike an earlier version
/// of this function, which wrote a second copy to a separate
/// `playback/{id}.mp4` and left the untouched HEVC original in place,
/// permanently doubling disk usage for every converted video. Mirrors the old
/// Python `MediaProcessor.ensure_browser_playback`, minus the duplication.
/// Returns `Ok(None)` when nothing needed to change (already playback-safe).
fn ensure_playback_copy(asset: &PendingAsset) -> Result<Option<PathBuf>, String> {
    let input = PathBuf::from(&asset.original_path);

    // A side-by-side copy from before this function transcoded in place: it's
    // already fully converted, so just fold it onto the original - no
    // re-encode, just a rename.
    if let Some(existing) = asset.playback_path.as_deref() {
        if existing != asset.original_path {
            let existing_path = PathBuf::from(existing);
            if !existing_path.is_file() {
                return Err(format!(
                    "previously-converted playback copy missing: {}",
                    existing_path.display()
                ));
            }
            std::fs::rename(&existing_path, &input).map_err(|err| {
                format!(
                    "failed to fold playback copy {} into {}: {err}",
                    existing_path.display(),
                    input.display()
                )
            })?;
            return Ok(Some(input));
        }
    }

    if !input.is_file() {
        return Err(format!("source file missing: {}", input.display()));
    }

    if !needs_playback_transcode(&input)? {
        return Ok(None);
    }

    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("asset");
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let extension_ok = input
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| PLAYBACK_SAFE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false);
    // Same directory as the source, not a separate `playback/` folder - the
    // whole point of transcoding in place is ending up with one file, not two.
    let final_path = if extension_ok { input.clone() } else { parent.join(format!("{stem}.mp4")) };
    // Must end in `.mp4`, not `.mp4.tmp` - ffmpeg picks its output muxer from
    // the destination filename's extension, and ".tmp" isn't a container
    // format it recognizes ("Unable to choose an output format").
    let tmp_path = parent.join(format!("{stem}.snapvault-tmp.mp4"));

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

    // `fs::rename` overwrites an existing destination on both Windows and
    // Unix, so when `final_path == input` this replaces the HEVC original
    // with the H.264 copy directly instead of leaving a second file behind.
    std::fs::rename(&tmp_path, &final_path)
        .map_err(|err| format!("failed to finalize playback copy {}: {err}", final_path.display()))?;

    if final_path != input {
        // Extension had to change (source wasn't .mp4/.m4v) - the original
        // still exists under its old name, now redundant.
        let _ = std::fs::remove_file(&input);
    }

    Ok(Some(final_path))
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
        assert_eq!(
            pending.iter().find(|a| a.id == "vid").unwrap().media_type,
            "video"
        );
    }

    #[test]
    fn has_video_stream_reports_missing_source_file() {
        let err = has_video_stream(Path::new("/does/not/exist.mp4")).unwrap_err();
        assert!(err.contains("ffprobe exited"));
    }

    #[test]
    fn generate_one_thumbnail_reports_missing_source_file() {
        let asset = PendingAsset {
            id: "missing".to_string(),
            media_type: "image".to_string(),
            original_path: "/does/not/exist.jpg".to_string(),
            overlay_path: None,
            playback_path: None,
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
    fn load_pending_assets_picks_up_old_side_by_side_playback_copies_for_migration() {
        // A video converted by the older side-by-side scheme has
        // playback_path pointing at a separate file, not original_path -
        // still needs picking up so it gets folded in and the duplicate
        // reclaimed, not left alone forever.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, thumbnail_path, playback_path)
             VALUES ('vid', 'video', '/b.mp4', '/b.webp', '/playback/vid.mp4')",
            [],
        )
        .unwrap();

        let pending = load_pending_assets(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(!pending[0].needs_thumbnail);
        assert!(pending[0].needs_playback);
        assert_eq!(pending[0].playback_path.as_deref(), Some("/playback/vid.mp4"));
    }

    #[test]
    fn ensure_playback_copy_transcodes_in_place_not_a_muxer_error() {
        // Regression test for a bug where the temp output path ended in
        // `.mp4.tmp` - ffmpeg picks its output muxer from the destination
        // filename's extension, so ".tmp" made every transcode fail with
        // "Unable to choose an output format", silently breaking playback
        // conversion for every video in the library.
        let tmp_dir = std::env::temp_dir().join(format!("snapvault-playback-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        // A tiny synthetic clip encoded as mpeg4 (not h264), so it needs transcoding.
        let input_path = tmp_dir.join("input.mp4");
        let gen = Command::new("ffmpeg")
            .args(["-y", "-f", "lavfi", "-i", "color=c=blue:s=64x64:d=1", "-c:v", "mpeg4", "-pix_fmt", "yuv420p"])
            .arg(&input_path)
            .output()
            .expect("failed to run ffmpeg to build test fixture (is it installed and on PATH?)");
        assert!(gen.status.success(), "fixture generation failed: {}", String::from_utf8_lossy(&gen.stderr));
        let original_size = std::fs::metadata(&input_path).unwrap().len();

        let asset = PendingAsset {
            id: "playback-test".to_string(),
            media_type: "video".to_string(),
            original_path: input_path.display().to_string(),
            overlay_path: None,
            playback_path: None,
            needs_thumbnail: false,
            needs_playback: true,
        };

        let result = ensure_playback_copy(&asset).unwrap();
        let final_path = result.expect("mpeg4 input should need a playback-safe transcode");

        // Transcoded in place: same path, no second file, content replaced.
        assert_eq!(final_path, input_path);
        assert!(final_path.is_file());
        assert_ne!(std::fs::metadata(&final_path).unwrap().len(), original_size);

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn ensure_playback_copy_folds_an_old_side_by_side_copy_in_without_reencoding() {
        let tmp_dir = std::env::temp_dir().join(format!("snapvault-playback-migrate-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let original_path = tmp_dir.join("original.mp4");
        std::fs::write(&original_path, b"stale hevc bytes").unwrap();
        let old_playback_path = tmp_dir.join("old-playback-copy.mp4");
        std::fs::write(&old_playback_path, b"already-converted h264 bytes").unwrap();

        let asset = PendingAsset {
            id: "migrate-test".to_string(),
            media_type: "video".to_string(),
            original_path: original_path.display().to_string(),
            overlay_path: None,
            playback_path: Some(old_playback_path.display().to_string()),
            needs_thumbnail: false,
            needs_playback: true,
        };

        let result = ensure_playback_copy(&asset).unwrap();
        let final_path = result.expect("should fold the side-by-side copy in");

        assert_eq!(final_path, original_path);
        assert!(!old_playback_path.is_file(), "old side-by-side copy should be gone (renamed, not copied)");
        assert_eq!(std::fs::read(&original_path).unwrap(), b"already-converted h264 bytes");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn ensure_playback_copy_reports_missing_source_file() {
        let asset = PendingAsset {
            id: "missing".to_string(),
            media_type: "video".to_string(),
            original_path: "/does/not/exist.mp4".to_string(),
            overlay_path: None,
            playback_path: None,
            needs_thumbnail: false,
            needs_playback: true,
        };
        let err = ensure_playback_copy(&asset).unwrap_err();
        assert!(err.contains("source file missing"));
    }
}

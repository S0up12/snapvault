use std::path::{Path, PathBuf};
use std::cell::RefCell;
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::db::DbState;
use crate::settings::{HwAccel, PerformanceSettings, TranscodeMode};

const PROGRESS_EVENT: &str = "thumbnails://progress";
const THUMBNAIL_SIZE: &str = "360:360";
const WEBP_QUALITY: &str = "72";
// Codecs WebView2/Chromium can decode without extra OS codec packs. Anything
// else (HEVC/H.265 is the common Snapchat-export case, but also covers .mov
// containers etc.) gets remuxed/transcoded into a playback-safe copy.
const PLAYBACK_SAFE_VIDEO_CODECS: &[&str] = &["h264"];
const PLAYBACK_SAFE_AUDIO_CODECS: &[&str] = &["aac", "mp3"];
const PLAYBACK_SAFE_EXTENSIONS: &[&str] = &["mp4", "m4v"];

/// Spawns `program` without flashing a console window. ffmpeg/ffprobe are
/// console-subsystem binaries, so a plain `Command::new` pops a visible
/// window on Windows every time this GUI app spawns one.
pub(crate) fn no_window_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

/// Pure part of `ffmpeg_program`: the sidecar next to the exe if it exists.
fn sidecar_in(exe_dir: &Path, name: &str, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let candidate = exe_dir.join(file);
    exists(&candidate).then_some(candidate)
}

/// Bundled sidecar (Tauri puts externalBin next to the app exe) when present,
/// else the bare name so PATH lookup keeps dev/CI working.
pub(crate) fn ffmpeg_program(name: &str) -> std::ffi::OsString {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().and_then(|dir| sidecar_in(dir, name, |p| p.is_file())))
        .map(PathBuf::into_os_string)
        .unwrap_or_else(|| name.into())
}

fn is_bundled(name: &str) -> bool {
    ffmpeg_program(name) != std::ffi::OsString::from(name)
}

/// Cheap availability probe: returns ffmpeg's first `-version` line plus
/// "(bundled)" or "(system)", or Err when it isn't installed / on PATH.
#[tauri::command]
pub fn check_ffmpeg() -> Result<String, String> {
    let output = no_window_command(&ffmpeg_program("ffmpeg"))
        .arg("-version")
        .output()
        .map_err(|err| format!("ffmpeg not found on PATH: {err}"))?;
    if !output.status.success() {
        return Err(format!("ffmpeg -version exited with {}", output.status));
    }
    let line = String::from_utf8_lossy(&output.stdout).lines().next().unwrap_or("ffmpeg").to_string();
    Ok(format!("{line} ({})", if is_bundled("ffmpeg") { "bundled" } else { "system" }))
}

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
    /// Run stopped by the user; `summary` counts what finished before the stop.
    Cancelled {
        summary: ThumbnailSummary,
    },
    Paused {
        processed: usize,
        total: usize,
    },
    Resumed {
        processed: usize,
        total: usize,
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
    pub cancelled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunState {
    Idle,
    Running,
    Paused,
    Cancelling,
}

impl RunState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Cancelling => "cancelling",
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Run-control state machine shared by the commands and the worker pool.
/// Idle -> Running (slot acquired) <-> Paused; Running/Paused -> Cancelling;
/// back to Idle when the slot drops. Pause is cooperative: workers block
/// between assets, so a file already being transcoded finishes first. Cancel
/// also kills the running ffmpeg/ffprobe children (see `run_output`).
pub struct MediaControl {
    state: Mutex<RunState>,
    cv: Condvar,
    children: Mutex<Vec<Arc<Mutex<Child>>>>,
    processed: AtomicUsize,
    total: AtomicUsize,
}

impl Default for MediaControl {
    fn default() -> Self {
        Self {
            state: Mutex::new(RunState::Idle),
            cv: Condvar::new(),
            children: Mutex::new(Vec::new()),
            processed: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
        }
    }
}

impl MediaControl {
    pub fn state(&self) -> RunState {
        *lock(&self.state)
    }

    fn is_cancelling(&self) -> bool {
        self.state() == RunState::Cancelling
    }

    fn try_start(&self) -> bool {
        let mut st = lock(&self.state);
        if *st != RunState::Idle {
            return false;
        }
        *st = RunState::Running;
        self.processed.store(0, Ordering::SeqCst);
        self.total.store(0, Ordering::SeqCst);
        true
    }

    fn finish(&self) {
        *lock(&self.state) = RunState::Idle;
        self.cv.notify_all();
    }

    fn transition(&self, from: &[RunState], to: RunState) -> Result<(), String> {
        let mut st = lock(&self.state);
        if !from.contains(&*st) {
            return Err(match *st {
                RunState::Idle => "No media processing is running.".to_string(),
                other => format!("Cannot do that while media processing is {}.", other.as_str()),
            });
        }
        *st = to;
        self.cv.notify_all();
        Ok(())
    }

    pub fn pause(&self) -> Result<(), String> {
        self.transition(&[RunState::Running], RunState::Paused)
    }

    pub fn resume(&self) -> Result<(), String> {
        self.transition(&[RunState::Paused], RunState::Running)
    }

    pub fn cancel(&self) -> Result<(), String> {
        self.transition(&[RunState::Running, RunState::Paused], RunState::Cancelling)?;
        for child in lock(&self.children).iter() {
            let _ = lock(child).kill();
        }
        Ok(())
    }

    /// Worker gate between assets: blocks (no CPU) while paused; false = stop.
    fn wait_for_turn(&self) -> bool {
        let mut st = lock(&self.state);
        loop {
            match *st {
                RunState::Paused => st = self.cv.wait(st).unwrap_or_else(|e| e.into_inner()),
                RunState::Cancelling => return false,
                _ => return true,
            }
        }
    }

    fn counts(&self) -> (usize, usize) {
        (self.processed.load(Ordering::SeqCst), self.total.load(Ordering::SeqCst))
    }
}

thread_local! {
    /// Set on each media worker so `run_output` can register its child.
    static ACTIVE_CTL: RefCell<Option<Arc<MediaControl>>> = const { RefCell::new(None) };
}

fn cancelled() -> bool {
    ACTIVE_CTL.with(|c| c.borrow().as_ref().is_some_and(|ctl| ctl.is_cancelling()))
}

/// Like `Command::output`, but on a media worker the child is registered so
/// `MediaControl::cancel` can kill it. Polls `try_wait` (5ms) because the
/// child must stay shareable while we wait.
fn run_output(cmd: &mut Command) -> std::io::Result<Output> {
    use std::io::Read;
    let Some(ctl) = ACTIVE_CTL.with(|c| c.borrow().clone()) else {
        return cmd.output();
    };
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    }
    let out_t = drain(child.stdout.take());
    let err_t = drain(child.stderr.take());
    let child = Arc::new(Mutex::new(child));
    lock(&ctl.children).push(child.clone());
    if ctl.is_cancelling() {
        // Cancel's kill pass may have run before we registered.
        let _ = lock(&child).kill();
    }
    let status = loop {
        match lock(&child).try_wait() {
            Ok(Some(s)) => break Ok(s),
            Ok(None) => {}
            Err(e) => break Err(e),
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    lock(&ctl.children).retain(|c| !Arc::ptr_eq(c, &child));
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();
    Ok(Output { status: status?, stdout, stderr })
}

/// Tracks whether `process_pending_media` is currently running, so the
/// startup auto-resume and a manual "Reprocess media" click can't overlap -
/// both would otherwise pull the same pending assets and race writing the
/// same `{id}.tmp.mp4` output paths.
#[derive(Default)]
pub struct MediaProcessingState {
    ctl: Arc<MediaControl>,
}

pub(crate) fn media_ctl(app: &AppHandle) -> Arc<MediaControl> {
    app.state::<MediaProcessingState>().ctl.clone()
}

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
    settings: &PerformanceSettings,
    ctl: &Arc<MediaControl>,
    emit: &dyn Fn(usize, usize, String),
) -> Result<ThumbnailSummary, String> {
    let thumbnail_dir = app_data_dir.join("thumbnails");
    std::fs::create_dir_all(&thumbnail_dir).map_err(|err| {
        format!(
            "failed to create thumbnails dir {}: {err}",
            thumbnail_dir.display()
        )
    })?;

    let mut pending = {
        let conn = db.lock().map_err(|err| err.to_string())?;
        load_pending_assets(&conn, app_data_dir)?
    };
    // On-open mode: transcodes happen in `ensure_playback`; those assets stay pending.
    if settings.transcode_mode == TranscodeMode::OnOpen {
        pending.retain_mut(|a| {
            a.needs_playback = false;
            a.needs_thumbnail
        });
    }
    let total = pending.len();
    ctl.total.store(total, Ordering::SeqCst);
    emit(0, total, "Processing media".to_string());

    if total == 0 {
        return Ok(ThumbnailSummary {
            total: 0,
            generated: 0,
            failed: 0,
            playback_transcoded: 0,
            playback_failed: 0,
            cancelled: false,
        });
    }

    // Same IPC-flooding guard as the ingestion progress events: cap updates
    // to ~200 regardless of library size.
    let emit_step = (total / 200).max(1);

    // Workers do the slow ffprobe/ffmpeg work with no DB access and send the
    // resulting DB ops here; this thread batches them into transactions and
    // emits progress, so `processed` is a monotonic completion count.
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let (tx, rx) = std::sync::mpsc::channel::<(String, Vec<Op>)>();
    let workers = settings.media_workers().min(total);
    let mut summary = ThumbnailSummary { total, generated: 0, failed: 0, playback_transcoded: 0, playback_failed: 0, cancelled: false };
    let mut batch: Vec<(String, Vec<Op>)> = Vec::new();
    let mut db_err: Option<String> = None;
    let mut processed = 0usize;

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let tx = tx.clone();
            let (next, stop, pending, thumbnail_dir) = (&next, &stop, &pending, &thumbnail_dir);
            let root = app_data_dir;
            let ctl = ctl.clone();
            scope.spawn(move || {
                ACTIVE_CTL.with(|c| *c.borrow_mut() = Some(ctl.clone()));
                while !stop.load(Ordering::Relaxed) {
                    if !ctl.wait_for_turn() {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(asset) = pending.get(i) else { break };
                    let ops = process_one(asset, db, root, thumbnail_dir, settings);
                    // Killed mid-asset: leave it pending (no ops, nothing marked failed).
                    if ctl.is_cancelling() && ops.iter().all(|o| !matches!(o, Op::Thumb(_) | Op::Playback { .. } | Op::Audio)) {
                        break;
                    }
                    if tx.send((asset.id.clone(), ops)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);

        for (id, ops) in rx {
            for op in &ops {
                match op {
                    Op::Thumb(_) => summary.generated += 1,
                    Op::ThumbFailed | Op::ThumbSpawnFailed => summary.failed += 1,
                    Op::Playback { .. } => summary.playback_transcoded += 1,
                    Op::PlaybackFailed { .. } => summary.playback_failed += 1,
                    Op::Audio | Op::PlaybackSafe(_) => {}
                }
            }
            batch.push((id, ops));
            processed += 1;
            ctl.processed.store(processed, Ordering::SeqCst);
            if batch.len() >= DB_BATCH && db_err.is_none() {
                if let Err(err) = flush_ops(db, &mut batch) {
                    db_err = Some(err);
                    stop.store(true, Ordering::Relaxed);
                }
            }
            if processed % emit_step == 0 || processed == total {
                emit(processed, total, format!("Processing media {processed}/{total}"));
            }
        }
    });
    if db_err.is_none() {
        if let Err(err) = flush_ops(db, &mut batch) {
            db_err = Some(err);
        }
    }
    summary.cancelled = ctl.is_cancelling() && processed < total;
    match db_err {
        Some(err) => Err(err),
        None => Ok(summary),
    }
}

const DB_BATCH: usize = 50;
static TRANSCODE_GATE: Mutex<()> = Mutex::new(());
/// On-open transcodes waiting for / holding the gate; background ones yield while > 0.
static PRIORITY_WAITERS: AtomicUsize = AtomicUsize::new(0);

struct PriorityWaiter;
impl PriorityWaiter {
    fn new() -> Self {
        PRIORITY_WAITERS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}
impl Drop for PriorityWaiter {
    fn drop(&mut self) {
        PRIORITY_WAITERS.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HwEncoder {
    Nvenc,
    Qsv,
    Amf,
}

/// Auto picks the first working one in this order.
const HW_ORDER: [HwEncoder; 3] = [HwEncoder::Nvenc, HwEncoder::Qsv, HwEncoder::Amf];

impl HwEncoder {
    fn id(self) -> &'static str {
        match self {
            Self::Nvenc => "nvenc",
            Self::Qsv => "qsv",
            Self::Amf => "amf",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Nvenc => "NVIDIA NVENC",
            Self::Qsv => "Intel Quick Sync",
            Self::Amf => "AMD AMF",
        }
    }
    fn ffmpeg_name(self) -> &'static str {
        match self {
            Self::Nvenc => "h264_nvenc",
            Self::Qsv => "h264_qsv",
            Self::Amf => "h264_amf",
        }
    }
    /// Quality-based video args (codec through pix_fmt).
    fn video_args(self) -> &'static [&'static str] {
        match self {
            Self::Nvenc => &["-c:v", "h264_nvenc", "-preset", "p4", "-cq", "23", "-pix_fmt", "yuv420p"],
            Self::Qsv => &["-c:v", "h264_qsv", "-global_quality", "23", "-preset", "medium", "-pix_fmt", "nv12"],
            Self::Amf => &[
                "-c:v", "h264_amf", "-quality", "balanced", "-rc", "cqp", "-qp_i", "23", "-qp_p", "23", "-qp_b", "23", "-pix_fmt", "yuv420p",
            ],
        }
    }
}

/// Pure encoder choice: `None` = libx264.
fn choose_encoder(pref: HwAccel, works: impl Fn(HwEncoder) -> bool) -> Option<HwEncoder> {
    let wanted = match pref {
        HwAccel::Off => return None,
        HwAccel::Auto => return HW_ORDER.into_iter().find(|e| works(*e)),
        HwAccel::Nvenc => HwEncoder::Nvenc,
        HwAccel::Qsv => HwEncoder::Qsv,
        HwAccel::Amf => HwEncoder::Amf,
    };
    works(wanted).then_some(wanted)
}

fn select_encoder(settings: &PerformanceSettings) -> Option<HwEncoder> {
    if settings.hw_accel == HwAccel::Off {
        return None;
    }
    if HW_BROKEN.load(Ordering::SeqCst) {
        return None;
    }
    let info = cached_hw_encoders().unwrap_or_default();
    let chosen = choose_encoder(settings.hw_accel, |e| info.iter().any(|i| i.id == e.id() && i.works));
    if chosen.is_none() && settings.hw_accel != HwAccel::Auto {
        crate::log::log_warn!("[thumbnails] requested hardware encoder unavailable, using libx264");
    }
    chosen
}

#[derive(Clone, Serialize, Debug)]
pub struct HwEncoderInfo {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub works: bool,
}

enum HwState {
    Unknown,
    Detecting,
    Ready(Vec<HwEncoderInfo>),
}
static HW_STATE: (Mutex<HwState>, Condvar) = (Mutex::new(HwState::Unknown), Condvar::new());

/// 1-second-ish synthetic encode; killed after 20s so a broken driver can't hang us.
fn test_encode(enc: HwEncoder) -> bool {
    let Ok(mut child) = no_window_command(&ffmpeg_program("ffmpeg"))
        .args(["-hide_banner", "-v", "error", "-f", "lavfi", "-i", "testsrc=s=256x256:r=10:d=1", "-frames:v", "10"])
        .args(enc.video_args())
        .args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Set when a hw transcode failed but libx264 succeeded; later assets skip hw
/// until `hw_encoders(force=true)` clears it.
static HW_BROKEN: AtomicBool = AtomicBool::new(false);

/// `ffmpeg -encoders` stdout, killed after 10s.
fn list_encoders() -> Result<String, String> {
    let mut child = no_window_command(&ffmpeg_program("ffmpeg"))
        .args(["-hide_banner", "-encoders"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("ffmpeg not found on PATH: {err}"))?;
    let mut stdout = child.stdout.take();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = stdout.as_mut() {
            let _ = std::io::Read::read_to_string(o, &mut s);
        }
        let _ = tx.send(s);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(s) => {
            let _ = child.wait();
            Ok(s)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err("ffmpeg -encoders timed out".to_string())
        }
    }
}

/// Runs detection with no lock held and stores the result (an empty list on
/// failure, so a broken ffmpeg isn't re-probed every transcode). The caller
/// must already have set the state to `Detecting`.
fn run_detection() -> Result<Vec<HwEncoderInfo>, String> {
    // Resets Detecting -> Ready(empty) if detection panics, so waiters never hang.
    struct Finish(Option<Vec<HwEncoderInfo>>);
    impl Drop for Finish {
        fn drop(&mut self) {
            *lock(&HW_STATE.0) = HwState::Ready(self.0.take().unwrap_or_default());
            HW_STATE.1.notify_all();
        }
    }
    let mut fin = Finish(None);
    let infos = list_encoders().map(|listing| {
        HW_ORDER
            .into_iter()
            .map(|e| {
                let available = listing.split_whitespace().any(|w| w == e.ffmpeg_name());
                HwEncoderInfo { id: e.id().into(), label: e.label().into(), available, works: available && test_encode(e) }
            })
            .collect::<Vec<_>>()
    });
    if let Ok(infos) = &infos {
        crate::log::log_info!(
            "[thumbnails] hw encoders working: {}",
            infos.iter().filter(|i| i.works).map(|i| i.id.as_str()).collect::<Vec<_>>().join(",")
        );
        fin.0 = Some(infos.clone());
    }
    infos
}

/// For the Settings command: cached for the session; `force` re-runs the tests.
/// Concurrent callers wait for the one running detection (never holds the lock
/// while ffmpeg runs).
fn hw_encoders(force: bool) -> Result<Vec<HwEncoderInfo>, String> {
    let mut state = lock(&HW_STATE.0);
    loop {
        match &*state {
            HwState::Detecting => state = HW_STATE.1.wait(state).unwrap_or_else(|e| e.into_inner()),
            HwState::Ready(c) if !force => return Ok(c.clone()),
            _ => break,
        }
    }
    *state = HwState::Detecting;
    drop(state);
    if force {
        HW_BROKEN.store(false, Ordering::SeqCst);
    }
    run_detection()
}

/// Non-blocking read for background work: `None` while unknown/detecting
/// (the first caller kicks off detection in a background thread).
fn cached_hw_encoders() -> Option<Vec<HwEncoderInfo>> {
    let mut state = lock(&HW_STATE.0);
    match &*state {
        HwState::Ready(c) => Some(c.clone()),
        HwState::Detecting => None,
        HwState::Unknown => {
            *state = HwState::Detecting;
            drop(state);
            std::thread::spawn(|| {
                let _ = run_detection();
            });
            None
        }
    }
}

#[tauri::command]
pub async fn detect_hw_encoders(force_refresh: Option<bool>) -> Result<Vec<HwEncoderInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || hw_encoders(force_refresh.unwrap_or(false)))
        .await
        .map_err(|err| format!("encoder detection panicked: {err}"))?
}

/// One ffmpeg transcode of `input` into `tmp_path` (removed on failure), one at a time.
fn run_transcode(input: &Path, tmp_path: &Path, settings: &PerformanceSettings, enc: Option<HwEncoder>) -> Result<(), String> {
    let mut command = no_window_command(&ffmpeg_program("ffmpeg"));
    command.arg("-y");
    if enc.is_some() {
        command.args(["-hwaccel", "auto"]);
    }
    command.arg("-i").arg(input).args(["-map", "0:v:0", "-map", "0:a:0?"]);
    match enc {
        Some(e) => {
            command.args(e.video_args());
        }
        None => {
            command.args(["-c:v", "libx264", "-preset", settings.transcode_preset.ffmpeg_preset(), "-crf", "23", "-pix_fmt", "yuv420p"]);
            if let Some(thread_cap) = settings.ffmpeg_thread_cap() {
                command.args(["-threads", &thread_cap.to_string()]);
            }
        }
    }
    command.args(["-movflags", "+faststart"]).args(["-c:a", "aac", "-b:a", "128k"]).arg(tmp_path);
    // At most one transcode at a time regardless of worker count.
    let gate = lock(&TRANSCODE_GATE);
    crate::log::log_info!("[thumbnails] transcoding with {}", enc.map_or("libx264", |e| e.id()));
    let output = run_output(&mut command).map_err(|err| format!("failed to run ffmpeg (is it installed and on PATH?): {err}"));
    drop(gate);
    let output = output?;
    if !output.status.success() {
        let _ = std::fs::remove_file(tmp_path);
        return Err(format!("ffmpeg exited with {}: {}", output.status, String::from_utf8_lossy(&output.stderr)));
    }
    Ok(())
}

#[derive(Clone, Serialize, Debug)]
pub struct PlaybackInfo {
    pub path: String,
    pub converted: bool,
}

type Inflight = Arc<(Mutex<Option<Result<PlaybackInfo, String>>>, Condvar)>;
static INFLIGHT: std::sync::LazyLock<Mutex<std::collections::HashMap<String, Inflight>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Transcode-on-open. Concurrent calls for one asset share a single run.
/// Ignores pause/cancel (user-initiated; runs on a plain thread, so run
/// control can't kill it). Process exit kills ffmpeg; a leftover tmp file is
/// overwritten by the next attempt.
fn ensure_playback_for(db: &Mutex<Connection>, root: &Path, settings: &PerformanceSettings, id: &str) -> Result<PlaybackInfo, String> {
    let (slot, leader) = {
        let mut map = lock(&INFLIGHT);
        match map.get(id) {
            Some(s) => (s.clone(), false),
            None => {
                let s: Inflight = Arc::new((Mutex::new(None), Condvar::new()));
                map.insert(id.to_string(), s.clone());
                (s, true)
            }
        }
    };
    if !leader {
        let mut res = lock(&slot.0);
        while res.is_none() {
            res = slot.1.wait(res).unwrap_or_else(|e| e.into_inner());
        }
        return res.clone().unwrap_or_else(|| Err("playback result missing".into()));
    }
    let _guard = InflightGuard { id, slot: &slot };
    let result = ensure_playback_run(db, root, settings, id);
    *lock(&slot.0) = Some(result.clone());
    result
}

/// Held by the leader; on drop (normal, error or panic) stores an Err if no
/// result was stored, removes the INFLIGHT entry and wakes waiters.
struct InflightGuard<'a> {
    id: &'a str,
    slot: &'a Inflight,
}
impl Drop for InflightGuard<'_> {
    fn drop(&mut self) {
        lock(&self.slot.0).get_or_insert_with(|| Err("playback run aborted".to_string()));
        lock(&INFLIGHT).remove(self.id);
        self.slot.1.notify_all();
    }
}

fn ensure_playback_run(db: &Mutex<Connection>, root: &Path, settings: &PerformanceSettings, id: &str) -> Result<PlaybackInfo, String> {
    let (media_type, original, playback): (String, String, Option<String>) = {
        let conn = db.lock().map_err(|err| err.to_string())?;
        conn.query_row("SELECT media_type, original_path, playback_path FROM assets WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map_err(|err| format!("asset not found: {err}"))?
    };
    let abs = |p: &str| crate::storage::to_absolute(root, p);
    let original_abs = abs(&original);
    let done = |path: &Path, converted| PlaybackInfo { path: path.display().to_string(), converted };
    if media_type != "video" {
        return Ok(done(&original_abs, false));
    }
    let playback_abs = playback.as_deref().map(abs).filter(|p| p.is_file());
    if let Some(p) = &playback_abs {
        return Ok(done(p, false));
    }
    let asset = PendingAsset {
        id: id.to_string(),
        media_type,
        original_path: original_abs.display().to_string(),
        overlay_path: None,
        playback_path: None,
        needs_thumbnail: false,
        needs_playback: true,
    };
    let copy = ensure_playback_copy(&asset, settings, true);
    if let Err(err) = &copy {
        if is_definitive_failure(err) {
            let mut batch = vec![(id.to_string(), vec![Op::PlaybackFailed { mark: true }])];
            let _ = flush_ops(db, &mut batch);
        }
    }
    match copy? {
        None => {
            let mut batch = vec![(id.to_string(), vec![Op::PlaybackSafe(original)])];
            flush_ops(db, &mut batch)?;
            Ok(done(&original_abs, false))
        }
        Some(final_path) => {
            let mut batch = vec![(
                id.to_string(),
                vec![Op::Playback {
                    final_path: crate::storage::to_relative(root, &final_path.display().to_string()),
                    old_path: (final_path != original_abs).then_some(original_abs),
                }],
            )];
            flush_ops(db, &mut batch)?;
            Ok(done(&final_path, true))
        }
    }
}

#[tauri::command]
pub async fn ensure_playback(app: AppHandle, asset_id: String) -> Result<PlaybackInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = crate::storage::resolve_media_root(&app)?;
        let settings = crate::settings::get_performance_settings(app.clone())?;
        ensure_playback_for(&app.state::<DbState>().0, &root, &settings, &asset_id)
    })
    .await
    .map_err(|err| format!("playback task panicked: {err}"))?
}

enum Op {
    Audio,
    Thumb(String),
    ThumbFailed,
    /// ffmpeg couldn't be spawned: counts as failed but isn't marked in the DB.
    ThumbSpawnFailed,
    /// `final_path` is DB-relative; `old_path` is the absolute pre-transcode
    /// file, `Some` only when the extension changed (deleted after commit).
    Playback { final_path: String, old_path: Option<PathBuf> },
    /// Original already playback-safe: record it as `playback_path` (DB-relative) so it isn't re-probed.
    PlaybackSafe(String),
    /// `mark`: definitive failure (corrupt/undecodable) -> `playback_failed = 1`; otherwise transient.
    PlaybackFailed { mark: bool },
}

/// Slow, DB-free work for one asset (ffprobe + ffmpeg); never aborts the run.
/// True when the DB now shows an in-place conversion (playback_path ==
/// original_path, file present), e.g. an on-open transcode finished after
/// this run loaded its snapshot. A legacy side-by-side copy has differing
/// paths and is not matched.
fn converted_meanwhile(db: &Mutex<Connection>, root: &Path, id: &str) -> bool {
    let Ok(conn) = db.lock() else { return false };
    let row: Option<(String, Option<String>)> =
        conn.query_row("SELECT original_path, playback_path FROM assets WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).ok();
    drop(conn);
    row.is_some_and(|(orig, pb)| pb.as_deref() == Some(orig.as_str()) && crate::storage::to_absolute(root, &orig).is_file())
}

/// ffprobe/ffmpeg ran and rejected the file (e.g. "moov atom not found"), as
/// opposed to a spawn failure or missing source, which may be transient.
fn is_definitive_failure(err: &str) -> bool {
    err.starts_with("ffprobe exited") || err.starts_with("ffmpeg exited")
}

fn process_one(asset: &PendingAsset, db: &Mutex<Connection>, root: &Path, thumbnail_dir: &Path, settings: &PerformanceSettings) -> Vec<Op> {
    let mut ops = Vec::new();
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
        ops.push(Op::Audio);
        return ops;
    }
    if cancelled() {
        return ops;
    }

    if asset.needs_thumbnail {
        let output_path = thumbnail_dir.join(format!("{}.webp", asset.id));
        match generate_one_thumbnail(asset, &output_path, settings) {
            Ok(()) => ops.push(Op::Thumb(crate::storage::to_relative(root, &output_path.display().to_string()))),
            Err(_) if cancelled() => {
                let _ = std::fs::remove_file(&output_path); // partial output
                return ops;
            }
            Err(err) => {
                crate::log::log_error!("[thumbnails] failed for asset {}: {err}", asset.id);
                // Marked so startup auto-resume doesn't retry forever; manual
                // generate_thumbnails clears the marker. A spawn failure
                // (ffmpeg missing) isn't a per-asset failure.
                ops.push(if err.contains("failed to run ffmpeg") { Op::ThumbSpawnFailed } else { Op::ThumbFailed });
            }
        }
    }

    if asset.needs_playback && !converted_meanwhile(db, root, &asset.id) {
        match ensure_playback_copy(asset, settings, false) {
            Ok(Some(final_path)) => ops.push(Op::Playback {
                final_path: crate::storage::to_relative(root, &final_path.display().to_string()),
                old_path: (final_path != Path::new(&asset.original_path)).then(|| PathBuf::from(&asset.original_path)),
            }),
            Ok(None) => ops.push(Op::PlaybackSafe(crate::storage::to_relative(root, &asset.original_path))),
            Err(_) if cancelled() => {} // tmp file already removed by ensure_playback_copy
            Err(_) if converted_meanwhile(db, root, &asset.id) => {} // on-open run got there first
            Err(err) => {
                crate::log::log_error!("[thumbnails] playback transcode failed for asset {}: {err}", asset.id);
                ops.push(Op::PlaybackFailed { mark: is_definitive_failure(&err) });
            }
        }
    }
    ops
}

/// Applies buffered ops in one transaction. Old originals are deleted only
/// after commit so the DB never points at a removed file.
fn flush_ops(db: &Mutex<Connection>, batch: &mut Vec<(String, Vec<Op>)>) -> Result<(), String> {
    if batch.is_empty() {
        return Ok(());
    }
    let mut conn = db.lock().map_err(|err| err.to_string())?;
    let tx = conn.transaction().map_err(|err| format!("failed to begin media batch: {err}"))?;
    let mut stale = Vec::new();
    for (id, ops) in batch.iter() {
        for op in ops {
            let r = match op {
                Op::Audio => tx.execute("UPDATE assets SET media_type = 'audio' WHERE id = ?1", [id]),
                Op::Thumb(path) => tx.execute(
                    "UPDATE assets SET thumbnail_path = ?1, thumb_failed = NULL WHERE id = ?2",
                    rusqlite::params![path, id],
                ),
                Op::ThumbFailed => tx.execute("UPDATE assets SET thumb_failed = 1 WHERE id = ?1", [id]),
                Op::Playback { final_path, old_path: None } => tx.execute(
                    "UPDATE assets SET playback_path = ?1 WHERE id = ?2",
                    rusqlite::params![final_path, id],
                ),
                Op::Playback { final_path, old_path: Some(old) } => {
                    // Extension changed (source wasn't .mp4/.m4v).
                    stale.push(old.clone());
                    tx.execute(
                        "UPDATE assets SET original_path = ?1, playback_path = ?1 WHERE id = ?2",
                        rusqlite::params![final_path, id],
                    )
                }
                Op::PlaybackSafe(path) => tx.execute(
                    "UPDATE assets SET playback_path = ?1 WHERE id = ?2",
                    rusqlite::params![path, id],
                ),
                Op::PlaybackFailed { mark: true } => {
                    crate::log::log_warn!("[thumbnails] asset {id} marked playback_failed (undecodable)");
                    tx.execute("UPDATE assets SET playback_failed = 1 WHERE id = ?1", [id])
                }
                Op::ThumbSpawnFailed | Op::PlaybackFailed { mark: false } => Ok(0),
            };
            r.map_err(|err| format!("failed to update asset {id}: {err}"))?;
        }
    }
    tx.commit().map_err(|err| format!("failed to commit media batch: {err}"))?;
    drop(conn);
    for path in stale {
        let _ = std::fs::remove_file(path);
    }
    batch.clear();
    Ok(())
}

/// Blocks (briefly polling) until no other `process_pending_media` run holds
/// `MediaProcessingState`, then claims it. `run_ingestion`'s media phase uses
/// this - unlike the manual command/startup resume below, it can't just bail
/// out with "already running" mid-import, so it waits its turn instead.
/// RAII guard: releases the slot on drop (incl. panic/early return).
pub(crate) struct MediaSlot(AppHandle);

impl Drop for MediaSlot {
    fn drop(&mut self) {
        self.0.state::<MediaProcessingState>().ctl.finish();
    }
}

impl MediaSlot {
    pub(crate) fn try_acquire(app: &AppHandle) -> Option<MediaSlot> {
        app.state::<MediaProcessingState>().ctl.try_start().then(|| MediaSlot(app.clone()))
    }

    /// Non-acquiring: true while any slot is held.
    pub(crate) fn is_busy(app: &AppHandle) -> bool {
        app.state::<MediaProcessingState>().is_busy()
    }
}

impl MediaProcessingState {
    fn is_busy(&self) -> bool {
        self.ctl.state() != RunState::Idle
    }
}

#[derive(Serialize)]
pub struct MediaStateView {
    pub state: &'static str,
    pub busy: bool,
}

#[tauri::command]
pub fn get_media_state(app: AppHandle) -> Result<MediaStateView, String> {
    let state = media_ctl(&app).state();
    Ok(MediaStateView { state: state.as_str(), busy: state != RunState::Idle })
}

/// Takes effect after each worker's current asset finishes (a long transcode
/// in flight completes first).
#[tauri::command]
pub fn pause_media_processing(app: AppHandle) -> Result<(), String> {
    let ctl = media_ctl(&app);
    ctl.pause()?;
    let (processed, total) = ctl.counts();
    let _ = app.emit(PROGRESS_EVENT, ProgressPayload::Paused { processed, total });
    Ok(())
}

#[tauri::command]
pub fn resume_media_processing(app: AppHandle) -> Result<(), String> {
    let ctl = media_ctl(&app);
    ctl.resume()?;
    let (processed, total) = ctl.counts();
    let _ = app.emit(PROGRESS_EVENT, ProgressPayload::Resumed { processed, total });
    Ok(())
}

/// Kills running ffmpeg/ffprobe children; the run then returns with
/// `cancelled: true` and emits status "cancelled".
#[tauri::command]
pub fn cancel_media_processing(app: AppHandle) -> Result<(), String> {
    media_ctl(&app).cancel()
}

/// True while an import's media phase or a thumbnail run holds the slot.
#[tauri::command]
pub fn get_media_activity(app: AppHandle) -> Result<bool, String> {
    Ok(MediaSlot::is_busy(&app))
}

/// Polls up to 1 hour, then gives up with Err.
pub(crate) fn acquire_media_processing_slot(app: &AppHandle) -> Result<MediaSlot, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3600);
    loop {
        if let Some(slot) = MediaSlot::try_acquire(app) {
            return Ok(slot);
        }
        if std::time::Instant::now() >= deadline {
            return Err("Timed out waiting for media processing slot.".to_string());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// Shared by the manual `generate_thumbnails` command and the startup
/// auto-resume task. Guards against both running at once (and against either
/// racing `run_ingestion`'s media phase - see `acquire_media_processing_slot`)
/// via `MediaProcessingState`, then runs `process_pending_media` on a
/// background thread (it shells out to ffmpeg/ffprobe per asset and waits on
/// each child process) and emits the same `thumbnails://progress` events
/// either way.
async fn run_and_emit(app: AppHandle, retry_failed: bool) -> Result<ThumbnailSummary, String> {
    let slot = MediaSlot::try_acquire(&app)
        .ok_or_else(|| "Media processing is already running.".to_string())?;
    if retry_failed {
        if let Ok(conn) = app.state::<DbState>().0.lock() {
            let _ = conn.execute("UPDATE assets SET thumb_failed = NULL, playback_failed = NULL", []);
        }
    }

    let app_for_blocking = app.clone();
    let result = tauri::async_runtime::spawn_blocking(
        move || -> Result<ThumbnailSummary, String> {
            let media_root = crate::storage::resolve_media_root(&app_for_blocking)?;
            let settings = crate::settings::get_performance_settings(app_for_blocking.clone())?;
            let state = app_for_blocking.state::<DbState>();

            let emit = |processed: usize, total: usize, message: String| {
                emit_progress(&app_for_blocking, processed, total, message);
            };
            process_pending_media(&state.0, &media_root, &settings, &media_ctl(&app_for_blocking), &emit)
        },
    )
    .await
    .map_err(|err| format!("thumbnail generation task panicked: {err}"));

    drop(slot);
    let result = result.and_then(|inner| inner);

    match &result {
        Ok(summary) => {
            let summary = summary.clone();
            let _ = app.emit(
                PROGRESS_EVENT,
                if summary.cancelled { ProgressPayload::Cancelled { summary } } else { ProgressPayload::Completed { summary } },
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
    // Manual run retries previously failed thumbnails.
    run_and_emit(app, true).await
}

/// Resumes any unfinished media processing left over from a prior run (the
/// app closing or crashing mid-batch, evidenced by orphaned `.tmp.mp4` files
/// and videos stuck without a `playback_path`) without waiting for someone to
/// notice and click "Reprocess media". Fire-and-forget: errors are logged,
/// not surfaced to any caller, since there isn't one at startup.
pub fn resume_pending_media_on_startup(app: AppHandle) {
    if !crate::settings::get_performance_settings(app.clone()).map(|s| s.process_on_startup).unwrap_or(true) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        if let Err(err) = run_and_emit(app, false).await {
            crate::log::log_error!("[thumbnails] startup auto-resume failed: {err}");
        }
    });
}

fn load_pending_assets(conn: &Connection, root: &Path) -> Result<Vec<PendingAsset>, String> {
    // `playback_path IS NOT NULL AND playback_path != original_path` catches
    // assets left over from the older side-by-side scheme (a separate
    // `playback/{id}.mp4`, HEVC original untouched) - `needs_playback` covers
    // both that migration case and "never processed at all" (NULL).
    let mut stmt = conn
        .prepare(
            "SELECT id, media_type, original_path, overlay_path, playback_path,
                    thumbnail_path IS NULL AND thumb_failed IS NULL,
                    media_type = 'video' AND playback_failed IS NULL AND (playback_path IS NULL OR playback_path != original_path)
             FROM assets
             WHERE media_type IN ('image', 'video')
               AND ((thumbnail_path IS NULL AND thumb_failed IS NULL)
                    OR (media_type = 'video' AND playback_failed IS NULL AND (playback_path IS NULL OR playback_path != original_path)))",
        )
        .map_err(|err| err.to_string())?;
    // PendingAsset carries absolute paths so all file/ffmpeg code is unchanged.
    let abs = |p: String| crate::storage::to_absolute(root, &p).display().to_string();
    let rows = stmt
        .query_map([], |row| {
            Ok(PendingAsset {
                id: row.get(0)?,
                media_type: row.get(1)?,
                original_path: abs(row.get::<_, String>(2)?),
                overlay_path: row.get::<_, Option<String>>(3)?.map(abs),
                playback_path: row.get::<_, Option<String>>(4)?.map(abs),
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
    let output = run_output(
        no_window_command(&ffmpeg_program("ffprobe"))
            .args([
                "-v", "error",
                "-select_streams", "v",
                "-show_entries", "stream=codec_type",
                "-of", "json",
            ])
            .arg(path),
    )
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
fn ensure_playback_copy(asset: &PendingAsset, settings: &PerformanceSettings, priority: bool) -> Result<Option<PathBuf>, String> {
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

    // Priority (on-open) requests count as waiters for the whole attempt;
    // background transcodes yield to them before queueing on the gate.
    let _waiter = priority.then(PriorityWaiter::new);
    if !priority {
        while PRIORITY_WAITERS.load(Ordering::SeqCst) > 0 && !cancelled() {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    let encoder = select_encoder(settings);
    let mut result = run_transcode(&input, &tmp_path, settings, encoder);
    if let (Err(err), Some(enc)) = (&result, encoder) {
        if !cancelled() {
            crate::log::log_warn!("[thumbnails] {} transcode failed, retrying with libx264: {}", enc.id(), err.lines().next().unwrap_or(""));
            result = run_transcode(&input, &tmp_path, settings, None);
            if result.is_ok() && !HW_BROKEN.swap(true, Ordering::SeqCst) {
                crate::log::log_warn!("[thumbnails] hardware encoding failed at runtime; using libx264 until encoders are re-detected");
            }
        }
    }
    result?;

    // `fs::rename` overwrites an existing destination on both Windows and
    // Unix, so when `final_path == input` this replaces the HEVC original
    // with the H.264 copy directly instead of leaving a second file behind.
    std::fs::rename(&tmp_path, &final_path)
        .map_err(|err| format!("failed to finalize playback copy {}: {err}", final_path.display()))?;

    // When the extension changed, the old original is NOT removed here: the
    // caller deletes it only after the DB row points at `final_path`, so a
    // crash in between leaves a valid original_path instead of a dangling one.
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

    let output = run_output(
        no_window_command(&ffmpeg_program("ffprobe"))
            .args(["-v", "error", "-show_entries", "stream=codec_name,codec_type", "-of", "json"])
            .arg(path),
    )
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

/// Grabs frame 0 (videos) or the source image itself (ffmpeg treats a still
/// image input as a single-frame video stream, so one code path covers both),
/// composites the caption/drawing overlay if present (mirrors the old
/// Python `MediaProcessor`'s PIL `alpha_composite` step), scales to fit
/// 360x360, and encodes as WebP. No `-ss` seek: it's a no-op for video (we
/// already want frame 0) and actively breaks the image2 demuxer for still
/// images, which drops the only frame and produces an empty file.
fn generate_one_thumbnail(asset: &PendingAsset, output_path: &Path, settings: &PerformanceSettings) -> Result<(), String> {
    let input = PathBuf::from(&asset.original_path);
    if !input.is_file() {
        return Err(format!("source file missing: {}", input.display()));
    }

    let overlay = asset
        .overlay_path
        .as_deref()
        .map(Path::new)
        .filter(|path| overlay_is_usable(path));

    if let Some(overlay) = overlay {
        match run_thumbnail_ffmpeg(&input, Some(overlay), output_path, settings) {
            Ok(()) => return Ok(()),
            Err(err) => crate::log::log_warn!(
                "[thumbnails] overlay composite failed for asset {}, retrying without overlay: {err}",
                asset.id
            ),
        }
    }
    run_thumbnail_ffmpeg(&input, None, output_path, settings)
}

/// Shared overlay graph: sizes the overlay to the base frame, then composites.
/// rw/rh size the overlay to the base frame (iw/ih were a no-op).
pub(crate) const OVERLAY_GRAPH: &str = "[1:v][0:v]scale2ref=w=rw:h=rh[ovr][base];[base][ovr]overlay=format=auto";

/// Overlay must exist, be non-trivial in size and start with the PNG magic
/// bytes; Snapchat occasionally exports truncated/non-PNG overlay files that
/// make ffmpeg fail with "Cannot determine format of input".
pub(crate) fn overlay_is_usable(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 8];
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    len >= 67 && file.read_exact(&mut magic).is_ok() && magic == *b"\x89PNG\r\n\x1a\n"
}

fn run_thumbnail_ffmpeg(
    input: &Path,
    overlay: Option<&Path>,
    output_path: &Path,
    settings: &PerformanceSettings,
) -> Result<(), String> {
    let mut command = no_window_command(&ffmpeg_program("ffmpeg"));
    command.args(["-y", "-i"]).arg(input);
    if let Some(thread_cap) = settings.ffmpeg_thread_cap() {
        command.args(["-threads", &thread_cap.to_string()]);
    }

    if let Some(overlay) = overlay {
        command.arg("-i").arg(overlay);
        // ponytail: scale2ref is deprecated in ffmpeg 7+ but still works;
        // the replacement (scale with a 2nd input) isn't in older builds.
        // rw/rh size the overlay to the base frame (iw/ih were a no-op).
        command.args([
            "-frames:v",
            "1",
            "-filter_complex",
            &format!(
                "{OVERLAY_GRAPH},scale={THUMBNAIL_SIZE}:force_original_aspect_ratio=decrease:flags=lanczos"
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

    let output = run_output(&mut command)
        .map_err(|err| format!("failed to run ffmpeg (is it installed and on PATH?): {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(6).collect();
        let tail = tail.into_iter().rev().collect::<Vec<_>>().join(" | ");
        return Err(format!(
            "ffmpeg exited with {} (inputs: {}{}): {tail}",
            output.status,
            input.display(),
            overlay.map(|o| format!(", {}", o.display())).unwrap_or_default(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_flag_tracks_slot_state() {
        // ponytail: tests the state the guard flips; AppHandle<Wry> can't be mocked here.
        let s = MediaProcessingState::default();
        assert!(!s.is_busy());
        assert!(s.ctl.try_start());
        assert!(!s.ctl.try_start());
        assert!(s.is_busy());
        s.ctl.finish();
        assert!(!s.is_busy());
    }

    #[test]
    fn load_pending_assets_excludes_audio_and_already_thumbnailed() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn, crate::db::MIGRATIONS, Path::new("/none")).unwrap();

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

        let pending = load_pending_assets(&conn, Path::new("/r")).unwrap();
        let ids: Vec<&str> = pending.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"img"));
        assert!(ids.contains(&"vid"));
        assert_eq!(
            pending.iter().find(|a| a.id == "vid").unwrap().media_type,
            "video"
        );
    }

    /// Bench: `cargo test bench_thumbnails -- --nocapture`. Skips without ffmpeg.
    #[test]
    fn bench_thumbnails() {
        if check_ffmpeg().is_err() {
            eprintln!("ffmpeg missing, skipping bench_thumbnails");
            return;
        }
        let dir = std::env::temp_dir().join(format!("snapvault-thumb-bench-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let seed = dir.join("seed.png");
        let gen = Command::new("ffmpeg")
            .args(["-y", "-f", "lavfi", "-i", "testsrc=s=640x480:d=1", "-frames:v", "1"])
            .arg(&seed)
            .output()
            .unwrap();
        assert!(gen.status.success());

        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn, crate::db::MIGRATIONS, Path::new("/none")).unwrap();
        const N: usize = 200;
        for i in 0..N {
            let p = dir.join(format!("{i}.png"));
            std::fs::copy(&seed, &p).unwrap();
            conn.execute(
                "INSERT INTO assets (id, media_type, original_path) VALUES (?1, 'image', ?2)",
                rusqlite::params![format!("a{i}"), p.display().to_string()],
            )
            .unwrap();
        }
        // One missing source: per-asset failure must not abort the run.
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path) VALUES ('bad', 'image', ?1)",
            rusqlite::params![dir.join("nope.png").display().to_string()],
        )
        .unwrap();
        let db = Mutex::new(conn);

        let last = std::sync::atomic::AtomicUsize::new(0);
        let emit = |processed: usize, _t: usize, _m: String| {
            assert!(processed >= last.swap(processed, Ordering::SeqCst), "progress not monotonic");
        };
        let start = std::time::Instant::now();
        let summary = process_pending_media(&db, &dir, &test_settings(), &Arc::new(MediaControl::default()), &emit).unwrap();
        let secs = start.elapsed().as_secs_f64();
        eprintln!("bench_thumbnails: {} assets in {secs:.2}s = {:.1}/s", N + 1, (N + 1) as f64 / secs);

        assert_eq!(summary.total, N + 1);
        assert_eq!(summary.generated, N);
        assert_eq!(summary.failed, 1);
        assert_eq!(last.load(Ordering::SeqCst), N + 1);
        let conn = db.lock().unwrap();
        let done: i64 = conn
            .query_row("SELECT COUNT(*) FROM assets WHERE thumbnail_path IS NOT NULL", [], |r| r.get(0))
            .unwrap();
        let marked: i64 = conn
            .query_row("SELECT COUNT(*) FROM assets WHERE thumb_failed = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!((done, marked), (N as i64, 1));
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_machine_transitions_and_errors() {
        let c = MediaControl::default();
        assert!(c.pause().is_err() && c.resume().is_err() && c.cancel().is_err());
        assert!(c.try_start());
        assert!(c.resume().is_err());
        c.pause().unwrap();
        assert_eq!(c.state(), RunState::Paused);
        c.resume().unwrap();
        c.pause().unwrap();
        c.cancel().unwrap();
        assert_eq!(c.state(), RunState::Cancelling);
        assert!(c.pause().is_err() && c.cancel().is_err());
        c.finish();
        assert_eq!(c.state(), RunState::Idle);
    }

    #[test]
    fn progress_payloads_serialize_status_strings() {
        let summary = ThumbnailSummary { total: 3, generated: 1, failed: 0, playback_transcoded: 0, playback_failed: 0, cancelled: true };
        let v = serde_json::to_value(ProgressPayload::Cancelled { summary }).unwrap();
        assert_eq!(v["status"], "cancelled");
        assert_eq!(v["summary"]["cancelled"], true);
        assert_eq!(v["summary"]["generated"], 1);
        let v = serde_json::to_value(ProgressPayload::Paused { processed: 2, total: 9 }).unwrap();
        assert_eq!((v["status"].as_str(), v["processed"].as_u64(), v["total"].as_u64()), (Some("paused"), Some(2), Some(9)));
        let v = serde_json::to_value(ProgressPayload::Resumed { processed: 2, total: 9 }).unwrap();
        assert_eq!(v["status"], "resumed");
    }

    /// Pause blocks workers, resume continues, cancel stops promptly leaving
    /// the rest pending (not failed). Skips without ffmpeg.
    #[test]
    fn pause_resume_cancel_control_a_run() {
        if check_ffmpeg().is_err() {
            eprintln!("ffmpeg missing, skipping pause_resume_cancel_control_a_run");
            return;
        }
        let dir = std::env::temp_dir().join(format!("snapvault-ctl-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let seed = dir.join("seed.png");
        let gen = Command::new("ffmpeg")
            .args(["-y", "-f", "lavfi", "-i", "testsrc=s=640x480:d=1", "-frames:v", "1"])
            .arg(&seed)
            .output()
            .unwrap();
        assert!(gen.status.success());
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn, crate::db::MIGRATIONS, Path::new("/none")).unwrap();
        const N: usize = 120;
        for i in 0..N {
            let p = dir.join(format!("{i}.png"));
            std::fs::copy(&seed, &p).unwrap();
            conn.execute(
                "INSERT INTO assets (id, media_type, original_path) VALUES (?1, 'image', ?2)",
                rusqlite::params![format!("a{i}"), p.display().to_string()],
            )
            .unwrap();
        }
        let db = Mutex::new(conn);
        let ctl = Arc::new(MediaControl::default());
        assert!(ctl.try_start());
        let done = AtomicBool::new(false);

        let summary = std::thread::scope(|s| {
            let h = s.spawn(|| {
                let r = process_pending_media(&db, &dir, &test_settings(), &ctl, &|_, _, _| {});
                done.store(true, Ordering::SeqCst);
                r
            });
            let wait_until = |f: &dyn Fn() -> bool| {
                let t = std::time::Instant::now();
                while !f() && t.elapsed().as_secs() < 30 {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            };
            wait_until(&|| ctl.counts().0 >= 3);
            ctl.pause().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(500)); // let in-flight assets drain
            let at_pause = ctl.counts().0;
            std::thread::sleep(std::time::Duration::from_millis(500));
            assert_eq!(ctl.counts().0, at_pause, "progress advanced while paused");
            assert!(!done.load(Ordering::SeqCst));
            ctl.resume().unwrap();
            wait_until(&|| ctl.counts().0 > at_pause);
            assert!(ctl.counts().0 > at_pause, "no progress after resume");
            ctl.pause().unwrap();
            ctl.cancel().unwrap(); // cancel while paused must wake the workers
            h.join().unwrap().unwrap()
        });
        assert!(summary.cancelled);
        let conn = db.lock().unwrap();
        let done_n: i64 = conn.query_row("SELECT COUNT(*) FROM assets WHERE thumbnail_path IS NOT NULL", [], |r| r.get(0)).unwrap();
        let failed: i64 = conn.query_row("SELECT COUNT(*) FROM assets WHERE thumb_failed = 1", [], |r| r.get(0)).unwrap();
        assert!((done_n as usize) < N, "cancel did not leave assets pending");
        assert_eq!(done_n as usize, summary.generated);
        assert_eq!(failed, 0);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
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
        let err = generate_one_thumbnail(&asset, Path::new("/tmp/out.webp"), &test_settings()).unwrap_err();
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
        crate::db::migrate(&conn, crate::db::MIGRATIONS, Path::new("/none")).unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, thumbnail_path) VALUES ('vid', 'video', '/b.mp4', '/b.webp')",
            [],
        )
        .unwrap();

        let pending = load_pending_assets(&conn, Path::new("/r")).unwrap();
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
        crate::db::migrate(&conn, crate::db::MIGRATIONS, Path::new("/none")).unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, thumbnail_path, playback_path)
             VALUES ('vid', 'video', '/b.mp4', '/b.webp', '/playback/vid.mp4')",
            [],
        )
        .unwrap();

        let pending = load_pending_assets(&conn, Path::new("/r")).unwrap();
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

        let result = ensure_playback_copy(&asset, &test_settings(), false).unwrap();
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

        let result = ensure_playback_copy(&asset, &test_settings(), false).unwrap();
        let final_path = result.expect("should fold the side-by-side copy in");

        assert_eq!(final_path, original_path);
        assert!(!old_playback_path.is_file(), "old side-by-side copy should be gone (renamed, not copied)");
        assert_eq!(std::fs::read(&original_path).unwrap(), b"already-converted h264 bytes");

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn choose_encoder_matrix() {
        use HwEncoder::*;
        let none = |_| false;
        let all = |_| true;
        let only_qsv = |e| e == Qsv;
        assert_eq!(choose_encoder(HwAccel::Off, all), None);
        assert_eq!(choose_encoder(HwAccel::Auto, all), Some(Nvenc));
        assert_eq!(choose_encoder(HwAccel::Auto, only_qsv), Some(Qsv));
        assert_eq!(choose_encoder(HwAccel::Auto, none), None);
        assert_eq!(choose_encoder(HwAccel::Amf, all), Some(Amf));
        assert_eq!(choose_encoder(HwAccel::Amf, only_qsv), None);
        assert_eq!(choose_encoder(HwAccel::Qsv, only_qsv), Some(Qsv));
        assert_eq!(choose_encoder(HwAccel::Nvenc, none), None);
    }

    /// Hardware off: conversion tests must not trigger real GPU detection or share its global state.
    fn test_settings() -> PerformanceSettings {
        PerformanceSettings { hw_accel: HwAccel::Off, ..Default::default() }
    }

    /// Fixture in a fresh dir plus a DB with one video row; None if ffmpeg/the codec is missing.
    fn video_fixture(codec_args: &[&str], ext: &str) -> Option<(PathBuf, Mutex<Connection>)> {
        let dir = std::env::temp_dir().join(format!("snapvault-onopen-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("v.{ext}"));
        let ok = Command::new("ffmpeg")
            .args(["-y", "-f", "lavfi", "-i", "color=c=blue:s=64x64:d=1"])
            .args(codec_args)
            .arg(&p)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("ffmpeg/codec missing, skipping");
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn, crate::db::MIGRATIONS, Path::new("/none")).unwrap();
        conn.execute(
            "INSERT INTO assets (id, media_type, original_path, thumbnail_path) VALUES ('v', 'video', ?1, 't.webp')",
            [p.display().to_string()],
        )
        .unwrap();
        Some((dir, Mutex::new(conn)))
    }

    #[test]
    fn ensure_playback_returns_immediately_for_safe_mp4() {
        let Some((dir, db)) = video_fixture(&["-c:v", "libx264", "-pix_fmt", "yuv420p"], "mp4") else { return };
        db.lock().unwrap().execute("UPDATE assets SET id = 'safe-mp4' WHERE id = 'v'", []).unwrap();
        let info = ensure_playback_for(&db, &dir, &test_settings(), "safe-mp4").unwrap();
        assert!(!info.converted);
        assert!(info.path.ends_with("v.mp4"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_playback_converts_hevc_and_coalesces() {
        let Some((dir, db)) = video_fixture(&["-c:v", "libx265", "-pix_fmt", "yuv420p"], "mp4") else { return };
        db.lock().unwrap().execute("UPDATE assets SET id = 'hevc' WHERE id = 'v'", []).unwrap();
        let s = test_settings();
        let (a, b) = std::thread::scope(|sc| {
            let h1 = sc.spawn(|| ensure_playback_for(&db, &dir, &s, "hevc"));
            let h2 = sc.spawn(|| ensure_playback_for(&db, &dir, &s, "hevc"));
            (h1.join().unwrap().unwrap(), h2.join().unwrap().unwrap())
        });
        assert_eq!(a.path, b.path);
        assert!(a.converted || b.converted);
        assert!(lock(&INFLIGHT).is_empty());
        let stored: String = db.lock().unwrap().query_row("SELECT playback_path FROM assets WHERE id='hevc'", [], |r| r.get(0)).unwrap();
        assert_eq!(stored, "v.mp4");
        assert!(!needs_playback_transcode(Path::new(&a.path)).unwrap());
        // Already converted: immediate, not converted again.
        assert!(!ensure_playback_for(&db, &dir, &s, "hevc").unwrap().converted);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn on_open_background_run_skips_transcodes_and_leaves_pending() {
        let Some((dir, db)) = video_fixture(&["-c:v", "mpeg4", "-pix_fmt", "yuv420p"], "mp4") else { return };
        let s = PerformanceSettings { transcode_mode: TranscodeMode::OnOpen, ..test_settings() };
        let summary = process_pending_media(&db, &dir, &s, &Arc::new(MediaControl::default()), &|_, _, _| {}).unwrap();
        assert_eq!((summary.total, summary.playback_transcoded, summary.playback_failed), (0, 0, 0));
        let pb: Option<String> = db.lock().unwrap().query_row("SELECT playback_path FROM assets WHERE id='v'", [], |r| r.get(0)).unwrap();
        assert!(pb.is_none());
        assert!(needs_playback_transcode(&dir.join("v.mp4")).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn inflight_guard_cleans_up_when_leader_panics() {
        let slot: Inflight = Arc::new((Mutex::new(None), Condvar::new()));
        lock(&INFLIGHT).insert("panic-id".into(), slot.clone());
        let s2 = slot.clone();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _g = InflightGuard { id: "panic-id", slot: &s2 };
            panic!("boom");
        }));
        assert!(r.is_err());
        assert!(!lock(&INFLIGHT).contains_key("panic-id"));
        assert!(matches!(&*lock(&slot.0), Some(Err(_))));
    }

    #[test]
    fn hw_cache_is_readable_while_detecting_and_latch_forces_libx264() {
        let s = PerformanceSettings { hw_accel: HwAccel::Nvenc, ..Default::default() };
        let prev = std::mem::replace(&mut *lock(&HW_STATE.0), HwState::Detecting);
        let t = std::time::Instant::now();
        assert!(cached_hw_encoders().is_none());
        assert_eq!(select_encoder(&s), None);
        assert!(t.elapsed() < std::time::Duration::from_secs(1));
        *lock(&HW_STATE.0) = prev;

        *lock(&HW_STATE.0) = HwState::Ready(vec![HwEncoderInfo { id: "nvenc".into(), label: String::new(), available: true, works: true }]);
        assert_eq!(select_encoder(&s), Some(HwEncoder::Nvenc));
        HW_BROKEN.store(true, Ordering::SeqCst);
        assert_eq!(select_encoder(&s), None);
        HW_BROKEN.store(false, Ordering::SeqCst);
        *lock(&HW_STATE.0) = HwState::Unknown;
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
        let err = ensure_playback_copy(&asset, &test_settings(), false).unwrap_err();
        assert!(err.contains("source file missing"));
    }

    #[test]
    fn sidecar_resolver_prefers_existing_exe_dir_file() {
        let dir = Path::new("app");
        let file = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        assert_eq!(sidecar_in(dir, "ffmpeg", |_| true), Some(dir.join(file)));
        assert_eq!(sidecar_in(dir, "ffmpeg", |_| false), None);
    }

    fn mem_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn, crate::db::MIGRATIONS, Path::new("/none")).unwrap();
        conn
    }

    #[test]
    fn definitive_failure_is_marked_and_not_pending_again() {
        let db = Mutex::new(mem_db());
        db.lock().unwrap().execute(
            "INSERT INTO assets (id, media_type, original_path, thumbnail_path) VALUES ('v', 'video', 'v.mp4', 't.webp')", []).unwrap();
        assert!(is_definitive_failure("ffprobe exited with exit code: 1: moov atom not found"));
        assert!(!is_definitive_failure("failed to run ffprobe (is it installed and on PATH?): x"));
        assert_eq!(load_pending_assets(&db.lock().unwrap(), Path::new("/r")).unwrap().len(), 1);
        flush_ops(&db, &mut vec![("v".into(), vec![Op::PlaybackFailed { mark: false }])]).unwrap();
        assert_eq!(load_pending_assets(&db.lock().unwrap(), Path::new("/r")).unwrap().len(), 1);
        flush_ops(&db, &mut vec![("v".into(), vec![Op::PlaybackFailed { mark: true }])]).unwrap();
        assert_eq!(load_pending_assets(&db.lock().unwrap(), Path::new("/r")).unwrap().len(), 0);
    }

    #[test]
    fn safe_original_is_recorded_as_playback_path() {
        let db = Mutex::new(mem_db());
        db.lock().unwrap().execute(
            "INSERT INTO assets (id, media_type, original_path, thumbnail_path) VALUES ('v', 'video', 'imports/v.mp4', 't.webp')", []).unwrap();
        flush_ops(&db, &mut vec![("v".into(), vec![Op::PlaybackSafe("imports/v.mp4".into())])]).unwrap();
        let pb: Option<String> = db.lock().unwrap().query_row("SELECT playback_path FROM assets WHERE id='v'", [], |r| r.get(0)).unwrap();
        assert_eq!(pb.as_deref(), Some("imports/v.mp4"));
        assert!(load_pending_assets(&db.lock().unwrap(), Path::new("/r")).unwrap().is_empty());
    }

    #[test]
    fn ensure_playback_records_safe_original_and_marks_corrupt() {
        let Some((dir, db)) = video_fixture(&["-c:v", "libx264", "-pix_fmt", "yuv420p"], "mp4") else { return };
        // Own id: INFLIGHT is process-global and sibling tests use "v".
        db.lock().unwrap().execute("UPDATE assets SET id = 'safe' WHERE id = 'v'", []).unwrap();
        ensure_playback_for(&db, &dir, &test_settings(), "safe").unwrap();
        let pb: Option<String> = db.lock().unwrap().query_row("SELECT playback_path FROM assets WHERE id='safe'", [], |r| r.get(0)).unwrap();
        assert!(pb.is_some_and(|p| p.ends_with("v.mp4")));
        // Corrupt file: truncated garbage with an mp4 extension.
        std::fs::write(dir.join("bad.mp4"), b"not a real mp4 at all").unwrap();
        db.lock().unwrap().execute(
            "INSERT INTO assets (id, media_type, original_path, thumbnail_path) VALUES ('bad', 'video', ?1, 't.webp')",
            [dir.join("bad.mp4").display().to_string()]).unwrap();
        assert!(ensure_playback_for(&db, &dir, &test_settings(), "bad").is_err());
        let f: Option<i64> = db.lock().unwrap().query_row("SELECT playback_failed FROM assets WHERE id='bad'", [], |r| r.get(0)).unwrap();
        assert_eq!(f, Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

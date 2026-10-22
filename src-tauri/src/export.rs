//! Export memories to a user-picked folder. One export at a time (own state
//! flag, never contends with `MediaSlot`). Logging is counts only.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::db::DbState;
use crate::ingestion::memories::{parse_iso_to_system_time, set_file_times};
use crate::thumbnails::{ffmpeg_program, no_window_command, overlay_is_usable, OVERLAY_GRAPH};

const EXPORT_EVENT: &str = "export://progress";
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
const COPY_CHUNK: usize = 1 << 20;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Organize {
    Flat,
    Year,
    #[default]
    YearMonth,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExportMemoriesOptions {
    pub destination: String,
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default = "yes")]
    pub apply_overlays: bool,
    #[serde(default)]
    pub organize: Organize,
    #[serde(default = "yes")]
    pub set_file_times: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ExportSummary {
    pub total: usize,
    pub exported: usize,
    pub composited: usize,
    pub overlay_failed: usize,
    pub skipped_existing: usize,
    pub missing_source: usize,
    pub failed: usize,
    pub videos_without_overlay: usize,
    pub cancelled: bool,
    pub bytes_written: u64,
    pub destination: String,
}

#[derive(Clone, Serialize)]
struct ProgressEvent<'a> {
    kind: &'static str,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    processed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<serde_json::Value>,
}

/// Run flag + cancel flag. Tauri-managed as `Arc<ExportCtl>`; tests make their own.
#[derive(Default, Debug)]
pub struct ExportCtl {
    running: AtomicBool,
    cancel: AtomicBool,
}

#[derive(Debug)]
struct ExportGuard(Arc<ExportCtl>);

impl ExportCtl {
    fn try_start(self: &Arc<Self>) -> Result<ExportGuard, String> {
        if self.running.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return Err("An export is already running.".to_string());
        }
        self.cancel.store(false, Ordering::SeqCst);
        Ok(ExportGuard(self.clone()))
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

impl Drop for ExportGuard {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::SeqCst);
    }
}

pub fn manage(app: &AppHandle) {
    app.manage(Arc::new(ExportCtl::default()));
}

#[derive(Debug)]
struct ExportAsset {
    id: String,
    media_type: String,
    original_path: String,
    overlay_path: Option<String>,
    taken_at: Option<String>,
}

/// Same selection as `list_memory_assets` (shared `memory_where`), oldest first.
fn select_memories(conn: &Connection, filter: &str, tag: Option<&str>, year: Option<i32>) -> Result<Vec<ExportAsset>, String> {
    let sql = format!(
        "SELECT id, media_type, original_path, overlay_path, taken_at FROM assets WHERE {} ORDER BY taken_at ASC, id ASC",
        crate::library::memory_where(filter, tag, year)
    );
    let fp = crate::library::FilterParams::new(tag, year);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(&fp.named()[..], |r| {
            Ok(ExportAsset {
                id: r.get(0)?,
                media_type: r.get(1)?,
                original_path: r.get(2)?,
                overlay_path: r.get(3)?,
                taken_at: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// Resolves symlinks/`\\?\` for the nearest existing ancestor so containment checks compare like with like.
fn normalize(p: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut cur = p.to_path_buf();
    loop {
        if let Ok(c) = fs::canonicalize(&cur) {
            return rest.iter().rev().fold(c, |acc, part| acc.join(part));
        }
        match (cur.file_name().map(|f| f.to_owned()), cur.parent().map(Path::to_path_buf)) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                cur = parent;
            }
            _ => return p.to_path_buf(),
        }
    }
}

fn validate_destination(dest: &Path, media_root: &Path, system: &[PathBuf], exact: &[PathBuf]) -> Result<(), String> {
    crate::storage::validate_media_root(dest, system, exact).map_err(|e| e.replace("Storage location", "Export folder"))?;
    if crate::storage::path_starts_with(&normalize(dest), &normalize(media_root)) {
        return Err("Export folder cannot be inside the SnapVault library folder.".to_string());
    }
    fs::create_dir_all(dest).map_err(|e| format!("failed to create export folder: {e}"))?;
    let probe = dest.join(format!(".snapvault-write-test-{}", std::process::id()));
    fs::write(&probe, b"x").map_err(|e| format!("Export folder is not writable: {e}"))?;
    let _ = fs::remove_file(&probe);
    Ok(())
}

fn safe_id8(id: &str) -> String {
    id.chars().take(8).map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

/// (relative dir, file stem) for an asset; undated when `taken_at` is NULL/unparsable.
fn export_name(asset: &ExportAsset, organize: Organize) -> (PathBuf, String) {
    let id8 = safe_id8(&asset.id);
    let dated = asset.taken_at.as_deref().filter(|t| parse_iso_to_system_time(t).is_some());
    let Some(t) = dated else {
        return (PathBuf::from("undated"), format!("undated_{id8}"));
    };
    let stem = format!("{}-{}-{}_{}{}{}_{id8}", &t[0..4], &t[5..7], &t[8..10], &t[11..13], &t[14..16], &t[17..19]);
    let dir = match organize {
        Organize::Flat => PathBuf::new(),
        Organize::Year => PathBuf::from(&t[0..4]),
        Organize::YearMonth => PathBuf::from(&t[0..4]).join(&t[5..7]),
    };
    (dir, stem)
}

/// Target path unique within this run (case-insensitive: Windows/macOS filesystems).
fn unique_target(dest: &Path, dir: &Path, stem: &str, ext: &str, used: &mut HashSet<String>) -> PathBuf {
    for n in 1u32.. {
        let name = if n == 1 { format!("{stem}.{ext}") } else { format!("{stem}_{n}.{ext}") };
        let path = dest.join(dir).join(name);
        if used.insert(path.to_string_lossy().to_lowercase()) {
            return path;
        }
    }
    unreachable!()
}

enum Step {
    Done,
    Cancelled,
}

fn temp_sibling(target: &Path, keep_ext: bool) -> PathBuf {
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = if keep_ext {
        // ffmpeg picks the muxer from the extension.
        let ext = target.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        format!("{}.snapvault-part.{ext}", name.trim_end_matches(&format!(".{ext}")))
    } else {
        format!("{name}.snapvault-part")
    };
    target.with_file_name(tmp)
}

/// Streaming copy to `tmp` then rename; checks `cancel` per chunk. Partial file removed on cancel/error.
fn copy_streaming(src: &Path, target: &Path, ctl: &ExportCtl) -> Result<(Step, u64), String> {
    let tmp = temp_sibling(target, false);
    let run = || -> std::io::Result<Option<u64>> {
        let mut input = fs::File::open(src)?;
        let mut out = fs::File::create(&tmp)?;
        let mut buf = vec![0u8; COPY_CHUNK];
        let mut total = 0u64;
        loop {
            if ctl.cancelled() {
                return Ok(None);
            }
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            total += n as u64;
        }
        out.flush()?;
        Ok(Some(total))
    };
    match run() {
        Ok(Some(n)) => fs::rename(&tmp, target).map(|_| (Step::Done, n)).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            e.to_string()
        }),
        Ok(None) => {
            let _ = fs::remove_file(&tmp);
            Ok((Step::Cancelled, 0))
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e.to_string())
        }
    }
}

/// Full-resolution composite into `tmp` (then renamed). Err = ffmpeg failed (caller falls back to copy).
fn composite(base: &Path, overlay: &Path, target: &Path, ctl: &ExportCtl) -> Result<(Step, u64), String> {
    let tmp = temp_sibling(target, true);
    let is_png = target.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"));
    let mut cmd = no_window_command(ffmpeg_program("ffmpeg"));
    cmd.args(["-y", "-v", "error", "-i"]).arg(base).arg("-i").arg(overlay);
    cmd.args(["-frames:v", "1", "-filter_complex", OVERLAY_GRAPH]);
    if !is_png {
        cmd.args(["-q:v", "2"]);
    }
    cmd.arg(&tmp).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| format!("failed to run ffmpeg: {e}"))?;
    let status = loop {
        if ctl.cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&tmp);
            return Ok((Step::Cancelled, 0));
        }
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                let _ = child.kill();
                let _ = fs::remove_file(&tmp);
                return Err(e.to_string());
            }
        }
    };
    let len = fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    if !status.success() || len == 0 {
        let _ = fs::remove_file(&tmp);
        return Err("ffmpeg composite failed".to_string());
    }
    fs::rename(&tmp, target).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })?;
    Ok((Step::Done, len))
}

fn stamp(target: &Path, taken_at: Option<&str>) {
    if let Some(t) = taken_at.and_then(parse_iso_to_system_time) {
        let _ = set_file_times(target, t);
    }
}

/// Pure-ish export loop (no Tauri): testable with a temp dir and a private `ExportCtl`.
fn export_assets(
    assets: &[ExportAsset],
    media_root: &Path,
    dest: &Path,
    opts: &ExportMemoriesOptions,
    ctl: &ExportCtl,
    mut on_progress: impl FnMut(usize, usize),
) -> ExportSummary {
    let mut s = ExportSummary { total: assets.len(), destination: dest.display().to_string(), ..Default::default() };
    let mut used = HashSet::new();
    for (i, a) in assets.iter().enumerate() {
        if ctl.cancelled() {
            s.cancelled = true;
            break;
        }
        on_progress(i, s.total);
        let src = crate::storage::to_absolute(media_root, &a.original_path);
        if !src.is_file() {
            s.missing_source += 1;
            continue;
        }
        let ext = src.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "bin".to_string());
        let (dir, stem) = export_name(a, opts.organize);
        let is_video = a.media_type == "video";
        if opts.apply_overlays && is_video && a.overlay_path.is_some() {
            s.videos_without_overlay += 1;
        }
        if let Err(e) = fs::create_dir_all(dest.join(&dir)) {
            crate::log::log_warn!("[export] cannot create folder: {e}");
            s.failed += 1;
            continue;
        }

        if opts.apply_overlays && !is_video {
            if let Some(ov) = a.overlay_path.as_deref() {
                let ov = crate::storage::to_absolute(media_root, ov);
                let out_ext = if ext.eq_ignore_ascii_case("png") { "png" } else { "jpg" };
                let target = unique_target(dest, &dir, &stem, out_ext, &mut used);
                let res = if overlay_is_usable(&ov) { composite(&src, &ov, &target, ctl) } else { Err("overlay unusable".into()) };
                match res {
                    Ok((Step::Cancelled, _)) => {
                        s.cancelled = true;
                        break;
                    }
                    Ok((Step::Done, n)) => {
                        if opts.set_file_times {
                            stamp(&target, a.taken_at.as_deref());
                        }
                        s.composited += 1;
                        s.exported += 1;
                        s.bytes_written += n;
                        continue;
                    }
                    Err(_) => {
                        s.overlay_failed += 1;
                        used.remove(&target.to_string_lossy().to_lowercase());
                    }
                }
            }
        }

        let target = unique_target(dest, &dir, &stem, &ext, &mut used);
        let src_len = fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
        if fs::metadata(&target).is_ok_and(|m| m.is_file() && m.len() == src_len) {
            s.skipped_existing += 1;
            continue;
        }
        match copy_streaming(&src, &target, ctl) {
            Ok((Step::Cancelled, _)) => {
                s.cancelled = true;
                break;
            }
            Ok((Step::Done, n)) => {
                if opts.set_file_times {
                    stamp(&target, a.taken_at.as_deref());
                }
                s.exported += 1;
                s.bytes_written += n;
            }
            Err(e) => {
                crate::log::log_warn!("[export] copy failed: {e}");
                s.failed += 1;
            }
        }
    }
    s
}

fn parse_year(year: Option<&str>) -> Result<Option<i32>, String> {
    match year.map(str::trim).filter(|y| !y.is_empty()) {
        None => Ok(None),
        Some(y) => y.parse::<i32>().map(Some).map_err(|_| "Invalid year.".to_string()),
    }
}

fn emit(app: &AppHandle, ev: ProgressEvent) {
    let _ = app.emit(EXPORT_EVENT, ev);
}

fn run_export(app: &AppHandle, ctl: &ExportCtl, opts: &ExportMemoriesOptions) -> Result<ExportSummary, String> {
    let media_root = crate::storage::resolve_media_root(app)?;
    let dest = PathBuf::from(opts.destination.trim());
    validate_destination(&dest, &media_root, &crate::storage::system_dirs(), &crate::storage::protected_exact_dirs())?;
    let year = parse_year(opts.year.as_deref())?;
    let tag = opts.tag.as_deref().filter(|t| !t.trim().is_empty());
    let filter = opts.filter.as_deref().unwrap_or("all");
    let assets = {
        let state = app.state::<DbState>();
        let conn = state.0.lock().map_err(|e| e.to_string())?;
        select_memories(&conn, filter, tag, year)?
    };
    crate::log::log_info!("[export] starting memories export of {} assets", assets.len());
    let mut last = Instant::now() - PROGRESS_INTERVAL;
    let summary = export_assets(&assets, &media_root, &dest, opts, ctl, |processed, total| {
        if last.elapsed() >= PROGRESS_INTERVAL {
            last = Instant::now();
            emit(app, ProgressEvent { kind: "memories", status: "progress", processed: Some(processed), total: Some(total), message: Some("Exporting memories"), summary: None });
        }
    });
    crate::log::log_info!(
        "[export] finished: exported={} composited={} overlay_failed={} skipped={} missing={} failed={} cancelled={}",
        summary.exported, summary.composited, summary.overlay_failed, summary.skipped_existing, summary.missing_source, summary.failed, summary.cancelled
    );
    Ok(summary)
}

/// Shared by both exports: single-flight guard, blocking thread, final event.
async fn run_guarded<T: Serialize + Send + 'static>(
    app: AppHandle,
    kind: &'static str,
    is_cancelled: fn(&T) -> bool,
    work: impl FnOnce(&AppHandle, &ExportCtl) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let ctl = app.state::<Arc<ExportCtl>>().inner().clone();
    let guard = ctl.try_start()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let result = work(&app, &ctl);
        match &result {
            Ok(s) => emit(&app, ProgressEvent {
                kind,
                status: if is_cancelled(s) { "cancelled" } else { "completed" },
                processed: None,
                total: None,
                message: None,
                summary: Some(serde_json::to_value(s).unwrap_or(serde_json::Value::Null)),
            }),
            Err(m) => emit(&app, ProgressEvent { kind, status: "error", processed: None, total: None, message: Some(m), summary: None }),
        }
        result
    })
    .await
    .map_err(|e| format!("export task failed: {e}"))?
}

#[tauri::command]
pub async fn export_memories(app: AppHandle, options: ExportMemoriesOptions) -> Result<ExportSummary, String> {
    run_guarded(app, "memories", |s: &ExportSummary| s.cancelled, move |app, ctl| run_export(app, ctl, &options)).await
}

#[tauri::command]
pub async fn export_chats(app: AppHandle, options: ExportChatsOptions) -> Result<ChatExportSummary, String> {
    run_guarded(app, "chats", |s: &ChatExportSummary| s.cancelled, move |app, ctl| run_chat_export(app, ctl, &options)).await
}

#[tauri::command]
pub fn cancel_export(app: AppHandle) -> Result<(), String> {
    let ctl = app.state::<Arc<ExportCtl>>();
    if !ctl.running.load(Ordering::SeqCst) {
        return Err("No export is running.".to_string());
    }
    ctl.cancel.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn get_export_state(app: AppHandle) -> Result<bool, String> {
    Ok(app.state::<Arc<ExportCtl>>().running.load(Ordering::SeqCst))
}

// ------------------------------------------------------------ chat export

const CHAT_PAGE: usize = 500;
const CANCEL_EVERY: usize = 1000;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatFormat {
    #[default]
    Html,
    Txt,
    Json,
}

impl ChatFormat {
    fn file(self) -> &'static str {
        match self {
            ChatFormat::Html => "chat.html",
            ChatFormat::Txt => "chat.txt",
            ChatFormat::Json => "chat.json",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExportChatsOptions {
    pub destination: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub format: ChatFormat,
    #[serde(default = "yes")]
    pub include_media: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ChatExportSummary {
    pub threads: usize,
    pub messages: usize,
    pub media_copied: usize,
    pub media_missing: usize,
    pub failed: usize,
    pub cancelled: bool,
    pub destination: String,
}

#[derive(Default)]
struct ThreadStats {
    messages: usize,
    media_copied: usize,
    media_missing: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum MediaKind {
    Image,
    Video,
    Other,
}

struct MediaRef {
    /// Path relative to the thread folder (`media/<name>`); None = not included / missing.
    rel: Option<String>,
    kind: MediaKind,
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// Windows-safe single path component: illegal/control chars -> `_`, no leading/trailing dots or
/// spaces, reserved device names prefixed, capped at 60 chars, never empty.
fn sanitize_component(raw: &str) -> String {
    let cleaned: String = raw.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).take(60).collect();
    let cleaned = cleaned.trim_matches(|c| c == '.' || c == ' ').to_string();
    if cleaned.is_empty() {
        return "chat".to_string();
    }
    let stem = cleaned.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let numbered = stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered {
        return format!("_{cleaned}");
    }
    cleaned
}

/// `2020-01-02T03:04:05.000Z` -> `2020-01-02 03:04:05 UTC`; anything else is passed through.
fn human_time(iso: &str) -> String {
    if iso.is_ascii() && iso.len() >= 19 && iso.as_bytes()[10] == b'T' {
        format!("{} {} UTC", &iso[..10], &iso[11..19])
    } else {
        iso.to_string()
    }
}

fn media_name(asset_id: &str, src: &Path) -> String {
    let id: String = asset_id.chars().take(64).map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let ext: String = src.extension().map(|e| e.to_string_lossy().chars().filter(char::is_ascii_alphanumeric).take(8).collect()).unwrap_or_default();
    if ext.is_empty() { format!("{id}.bin") } else { format!("{id}.{ext}") }
}

const HTML_CSS: &str = "*{box-sizing:border-box}body{margin:0;padding:16px;font:15px/1.4 system-ui,sans-serif;background:#f4f4f6;color:#16161a}\
main{max-width:720px;margin:0 auto}h1{font-size:20px}.day{text-align:center;margin:18px 0 8px;font-size:12px;opacity:.6}\
.row{display:flex;margin:4px 0}.row.me{justify-content:flex-end}.bubble{max-width:80%;padding:8px 12px;border-radius:14px;background:#fff}\
.me .bubble{background:#2d6cdf;color:#fff}.meta{font-size:11px;opacity:.65;margin-bottom:2px}.text{white-space:pre-wrap;overflow-wrap:anywhere}\
img,video{display:block;max-width:100%;max-height:360px;margin-top:6px;border-radius:8px}a{color:inherit}\
@media(prefers-color-scheme:dark){body{background:#101014;color:#e8e8ee}.bubble{background:#23232b}.me .bubble{background:#2d6cdf;color:#fff}}";

fn write_head(w: &mut impl Write, fmt: ChatFormat, thread_id: &str, title: &str, now: &str) -> std::io::Result<()> {
    match fmt {
        ChatFormat::Html => write!(
            w,
            "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
             <title>{t}</title><style>{HTML_CSS}</style></head><body><main><h1>{t}</h1>\n",
            t = esc(title)
        ),
        ChatFormat::Txt => write!(w, "Chat: {title}\nExported: {now}\n\n"),
        ChatFormat::Json => write!(
            w,
            "{{\"thread\":{},\"exported_at\":{},\"messages\":[",
            serde_json::json!({ "id": thread_id, "title": title }),
            serde_json::Value::String(now.to_string())
        ),
    }
}

fn write_foot(w: &mut impl Write, fmt: ChatFormat) -> std::io::Result<()> {
    match fmt {
        ChatFormat::Html => writeln!(w, "</main></body></html>"),
        ChatFormat::Txt => Ok(()),
        ChatFormat::Json => writeln!(w, "]}}"),
    }
}

fn write_message(
    w: &mut impl Write,
    fmt: ChatFormat,
    m: &crate::chats::ChatMessageView,
    refs: &[MediaRef],
    first: bool,
    last_day: &mut String,
) -> std::io::Result<()> {
    let text = m.body.as_deref().filter(|b| !b.trim().is_empty());
    match fmt {
        ChatFormat::Json => {
            let media: Vec<&str> = refs.iter().filter_map(|r| r.rel.as_deref()).collect();
            let v = serde_json::json!({ "id": m.id, "sent_at": m.sent_at, "sender": m.sender_label, "is_sender": m.is_me, "text": text, "media": media });
            write!(w, "{}{v}", if first { "" } else { "," })
        }
        ChatFormat::Txt => {
            writeln!(w, "[{}] {}: {}", human_time(&m.sent_at), m.sender_label, text.unwrap_or(""))?;
            for r in refs {
                match &r.rel {
                    Some(p) => writeln!(w, "    [media] {p}")?,
                    None => writeln!(w, "    [attachment not included]")?,
                }
            }
            Ok(())
        }
        ChatFormat::Html => {
            let day = m.sent_at.get(..10).unwrap_or("").to_string();
            if day != *last_day {
                writeln!(w, "<div class=\"day\">{}</div>", esc(&day))?;
                *last_day = day;
            }
            write!(
                w,
                "<div class=\"row {}\"><div class=\"bubble\"><div class=\"meta\">{} &middot; <time datetime=\"{}\">{}</time></div>",
                if m.is_me { "me" } else { "them" },
                esc(&m.sender_label),
                esc(&m.sent_at),
                esc(&human_time(&m.sent_at))
            )?;
            if let Some(t) = text {
                write!(w, "<div class=\"text\">{}</div>", esc(t))?;
            }
            for r in refs {
                match (&r.rel, r.kind) {
                    (Some(p), MediaKind::Image) => write!(w, "<a href=\"{p}\"><img loading=\"lazy\" src=\"{p}\" alt=\"\"></a>", p = esc(p))?,
                    (Some(p), MediaKind::Video) => write!(w, "<video controls preload=\"none\" src=\"{}\"></video>", esc(p))?,
                    (Some(p), MediaKind::Other) => write!(w, "<div class=\"text\"><a href=\"{p}\">{p}</a></div>", p = esc(p))?,
                    (None, _) => write!(w, "<div class=\"text\">[attachment not included]</div>")?,
                }
            }
            writeln!(w, "</div></div>")
        }
    }
}

/// Copies one asset into `<dir>/media/`. Ok(None) = cancelled; Ok(Some(None)) = missing/failed.
fn copy_chat_media(
    media_root: &Path,
    dir: &Path,
    med: &crate::chats::ChatMessageMedia,
    ctl: &ExportCtl,
    st: &mut ThreadStats,
    copied: &mut Vec<PathBuf>,
) -> Result<Option<Option<String>>, String> {
    let src = crate::storage::to_absolute(media_root, &med.original_path);
    if !src.is_file() {
        st.media_missing += 1;
        return Ok(Some(None));
    }
    let name = media_name(&med.id, &src);
    fs::create_dir_all(dir.join("media")).map_err(|e| e.to_string())?;
    let target = dir.join("media").join(&name);
    match copy_streaming(&src, &target, ctl) {
        Ok((Step::Cancelled, _)) => Ok(None),
        Ok((Step::Done, _)) => {
            copied.push(target);
            st.media_copied += 1;
            Ok(Some(Some(format!("media/{name}"))))
        }
        Err(e) => {
            crate::log::log_warn!("[export] chat media copy failed: {e}");
            st.media_missing += 1;
            Ok(Some(None))
        }
    }
}

struct ThreadJob<'a> {
    id: &'a str,
    title: &'a str,
    dir: &'a Path,
    tmp: &'a Path,
}

#[allow(clippy::too_many_arguments)]
fn write_thread(
    db: &Mutex<Connection>,
    job: &ThreadJob,
    media_root: &Path,
    opts: &ExportChatsOptions,
    ctl: &ExportCtl,
    now: &str,
    st: &mut ThreadStats,
    copied: &mut Vec<PathBuf>,
    progress: &mut dyn FnMut(),
) -> Result<Step, String> {
    let io = |e: std::io::Error| e.to_string();
    let fmt = opts.format;
    let mut w = std::io::BufWriter::new(fs::File::create(job.tmp).map_err(io)?);
    write_head(&mut w, fmt, job.id, job.title, now).map_err(io)?;
    let mut assets: HashMap<String, Option<String>> = HashMap::new();
    let (mut cursor, mut last_day, mut seen) = (None::<String>, String::new(), 0usize);
    loop {
        let (msgs, next) = {
            let conn = db.lock().map_err(|e| e.to_string())?;
            crate::chats::messages_after(&conn, job.id, cursor.as_deref(), CHAT_PAGE)?
        };
        for m in &msgs {
            if seen % CANCEL_EVERY == 0 && ctl.cancelled() {
                return Ok(Step::Cancelled);
            }
            let mut refs = Vec::with_capacity(m.media.len());
            for med in &m.media {
                let kind = match med.media_type.as_str() {
                    "image" => MediaKind::Image,
                    "video" => MediaKind::Video,
                    _ => MediaKind::Other,
                };
                let rel = match (opts.include_media, assets.get(&med.id)) {
                    (false, _) => None,
                    (true, Some(r)) => r.clone(),
                    (true, None) => {
                        let Some(r) = copy_chat_media(media_root, job.dir, med, ctl, st, copied)? else { return Ok(Step::Cancelled) };
                        assets.insert(med.id.clone(), r.clone());
                        r
                    }
                };
                refs.push(MediaRef { rel, kind });
            }
            write_message(&mut w, fmt, m, &refs, seen == 0, &mut last_day).map_err(io)?;
            seen += 1;
            st.messages += 1;
            progress();
        }
        match next {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    write_foot(&mut w, fmt).map_err(io)?;
    w.flush().map_err(io)?;
    Ok(Step::Done)
}

/// Removes everything this run wrote for a thread (the folder too if we created it).
fn cleanup_thread(created: bool, dir: &Path, tmp: &Path, copied: &[PathBuf]) {
    let _ = fs::remove_file(tmp);
    for f in copied {
        let _ = fs::remove_file(f);
    }
    let _ = fs::remove_dir(dir.join("media"));
    if created {
        let _ = fs::remove_dir_all(dir);
    }
}

/// Pure export loop (no Tauri). The DB lock is taken per page, never across file writes.
fn export_chats_core(
    db: &Mutex<Connection>,
    media_root: &Path,
    dest: &Path,
    opts: &ExportChatsOptions,
    ctl: &ExportCtl,
    now: &str,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<ChatExportSummary, String> {
    type ThreadRow = (String, Option<String>, String);
    let (threads, total): (Vec<ThreadRow>, usize) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let tid = opts.thread_id.as_deref();
        let threads = conn
            .prepare("SELECT id, title, external_id FROM chat_threads WHERE (?1 IS NULL OR id = ?1) ORDER BY id")
            .map_err(|e| e.to_string())?
            .query_map([tid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        if tid.is_some() && threads.is_empty() {
            return Err("Chat thread not found.".to_string());
        }
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM chat_messages WHERE (?1 IS NULL OR thread_id = ?1)", [tid], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        (threads, total.max(0) as usize)
    };
    let mut s = ChatExportSummary { destination: dest.display().to_string(), ..Default::default() };
    let mut used = HashSet::new();
    let mut processed = 0usize;
    for (tid, title, external_id) in threads {
        if ctl.cancelled() {
            s.cancelled = true;
            break;
        }
        let display = title.filter(|t| !t.trim().is_empty()).unwrap_or(external_id);
        let mut name = format!("{}-{}", sanitize_component(&display), safe_id8(&tid));
        while !used.insert(name.to_lowercase()) {
            name.push('_'); // ponytail: only reachable if two ids share an 8-char prefix and title
        }
        let dir = dest.join(&name);
        let created = !dir.exists();
        let tmp = dir.join(format!("{}.snapvault-part", opts.format.file()));
        let (mut st, mut copied) = (ThreadStats::default(), Vec::new());
        let job = ThreadJob { id: &tid, title: &display, dir: &dir, tmp: &tmp };
        let res = fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| {
            write_thread(db, &job, media_root, opts, ctl, now, &mut st, &mut copied, &mut || {
                processed += 1;
                on_progress(processed, total);
            })
        });
        match res.and_then(|step| match step {
            Step::Done => fs::rename(&tmp, dir.join(opts.format.file())).map(|_| true).map_err(|e| e.to_string()),
            Step::Cancelled => Ok(false),
        }) {
            Ok(true) => {
                s.threads += 1;
                s.messages += st.messages;
                s.media_copied += st.media_copied;
                s.media_missing += st.media_missing;
            }
            Ok(false) => {
                cleanup_thread(created, &dir, &tmp, &copied);
                s.cancelled = true;
                break;
            }
            Err(e) => {
                crate::log::log_warn!("[export] chat thread failed: {e}");
                cleanup_thread(created, &dir, &tmp, &copied);
                s.failed += 1;
            }
        }
    }
    Ok(s)
}

#[allow(clippy::too_many_arguments)]
fn chat_export_checked(
    db: &Mutex<Connection>,
    media_root: &Path,
    opts: &ExportChatsOptions,
    ctl: &ExportCtl,
    now: &str,
    system: &[PathBuf],
    exact: &[PathBuf],
    on_progress: impl FnMut(usize, usize),
) -> Result<ChatExportSummary, String> {
    let dest = PathBuf::from(opts.destination.trim());
    validate_destination(&dest, media_root, system, exact)?;
    export_chats_core(db, media_root, &dest, opts, ctl, now, on_progress)
}

fn run_chat_export(app: &AppHandle, ctl: &ExportCtl, opts: &ExportChatsOptions) -> Result<ChatExportSummary, String> {
    let media_root = crate::storage::resolve_media_root(app)?;
    let now = crate::log::utc_timestamp(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    crate::log::log_info!("[export] starting chats export (all_threads={})", opts.thread_id.is_none());
    let mut last = Instant::now() - PROGRESS_INTERVAL;
    let state = app.state::<DbState>();
    let summary = chat_export_checked(
        &state.0,
        &media_root,
        opts,
        ctl,
        &now,
        &crate::storage::system_dirs(),
        &crate::storage::protected_exact_dirs(),
        |processed, total| {
            if last.elapsed() >= PROGRESS_INTERVAL {
                last = Instant::now();
                emit(app, ProgressEvent { kind: "chats", status: "progress", processed: Some(processed), total: Some(total), message: Some("Exporting chats"), summary: None });
            }
        },
    )?;
    crate::log::log_info!(
        "[export] chats finished: threads={} messages={} media_copied={} media_missing={} failed={} cancelled={}",
        summary.threads, summary.messages, summary.media_copied, summary.media_missing, summary.failed, summary.cancelled
    );
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::SCHEMA_SQL;
    use std::time::SystemTime;
    use uuid::Uuid;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("snapvault-export-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn opts(dest: &Path) -> ExportMemoriesOptions {
        serde_json::from_value(serde_json::json!({ "destination": dest.display().to_string() })).unwrap()
    }

    fn ffmpeg_ok() -> bool {
        no_window_command(ffmpeg_program("ffmpeg")).arg("-version").output().is_ok_and(|o| o.status.success())
    }

    fn asset(id: &str, taken: Option<&str>, path: &str) -> ExportAsset {
        ExportAsset { id: id.into(), media_type: "image".into(), original_path: path.into(), overlay_path: None, taken_at: taken.map(String::from) }
    }

    fn run(assets: &[ExportAsset], root: &Path, dest: &Path, o: &ExportMemoriesOptions) -> ExportSummary {
        export_assets(assets, root, dest, o, &ExportCtl::default(), |_, _| {})
    }

    #[test]
    fn options_defaults() {
        let o = opts(Path::new("/x"));
        assert!(o.apply_overlays && o.set_file_times);
        assert_eq!(o.organize, Organize::YearMonth);
        assert!(o.filter.is_none() && o.tag.is_none() && o.year.is_none());
    }

    #[test]
    fn filter_parity_with_list() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        let rows = [
            ("a", "image", "2019-06-01T10:00:00.000Z", 0, "[\"trip\"]", "memory"),
            ("b", "video", "2020-06-01T10:00:00.000Z", 1, "[\"trip\",\"x\"]", "memory"),
            ("c", "image", "2020-12-01T10:00:00.000Z", 1, "[]", "memory"),
            ("d", "image", "2021-01-01T10:00:00.000Z", 0, "[\"x\"]", "memory"),
            ("e", "image", "2020-01-01T10:00:00.000Z", 1, "[\"trip\"]", "chat"),
        ];
        for (id, mt, ts, fav, tags, src) in rows {
            conn.execute(
                "INSERT INTO assets (id, media_type, original_path, taken_at, is_favorite, tags, source_type) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                rusqlite::params![id, mt, format!("{id}.jpg"), ts, fav, tags, src],
            )
            .unwrap();
        }
        for filter in ["all", "photo", "video", "favorite"] {
            for tag in [None, Some("trip"), Some("x")] {
                for year in [None, Some(2019), Some(2020), Some(2021)] {
                    let listed = crate::library::list_for_test(&conn, filter, tag, year);
                    let exported: Vec<String> = select_memories(&conn, filter, tag, year).unwrap().into_iter().map(|a| a.id).collect();
                    assert_eq!(listed, exported, "{filter} {tag:?} {year:?}");
                }
            }
        }
        assert_eq!(select_memories(&conn, "all", None, None).unwrap().len(), 4);
    }

    #[test]
    fn names_collisions_and_undated() {
        let a = asset("abcdef123456", Some("2020-03-04T05:06:07.000Z"), "x.jpg");
        assert_eq!(export_name(&a, Organize::YearMonth), (PathBuf::from("2020").join("03"), "2020-03-04_050607_abcdef12".into()));
        assert_eq!(export_name(&a, Organize::Year).0, PathBuf::from("2020"));
        assert_eq!(export_name(&a, Organize::Flat).0, PathBuf::new());
        let u = asset("zz/..9999", None, "x.jpg");
        assert_eq!(export_name(&u, Organize::YearMonth), (PathBuf::from("undated"), "undated_zz___999".into()));
        let mut used = HashSet::new();
        let d = Path::new("d");
        let p1 = unique_target(d, Path::new(""), "s", "jpg", &mut used);
        let p2 = unique_target(d, Path::new(""), "S", "jpg", &mut used);
        assert_ne!(p1, p2);
        assert!(p2.to_string_lossy().ends_with("S_2.jpg"));
    }

    #[test]
    fn destination_validation() {
        let root = tmp();
        let ok = tmp();
        let sys = tmp();
        let sysdirs = vec![sys.clone()];
        assert!(validate_destination(&ok.join("out"), &root, &sysdirs, &[]).is_ok());
        assert!(validate_destination(&root, &root, &sysdirs, &[]).unwrap_err().contains("inside"));
        assert!(validate_destination(&root.join("sub"), &root, &sysdirs, &[]).unwrap_err().contains("inside"));
        assert!(!root.join("sub").exists());
        assert!(validate_destination(&sys.join("o"), &root, &sysdirs, &[]).is_err());
        assert!(validate_destination(&ok, &root, &[], &[ok.clone()]).is_err());
        assert!(validate_destination(Path::new("rel"), &root, &[], &[]).is_err());
        assert!(validate_destination(&ok.join("a").join("..").join("b"), &root, &[], &[]).is_err());
        // real system dirs
        assert!(validate_destination(&PathBuf::from(std::env::var_os("SystemRoot").unwrap_or("/usr".into())).join("x"), &root, &crate::storage::system_dirs(), &[]).is_err());
    }

    #[test]
    fn export_idempotent_missing_and_times() {
        let root = tmp();
        let dest = tmp().join("out");
        fs::write(root.join("a.jpg"), b"hello").unwrap();
        fs::write(root.join("b.mp4"), b"vid").unwrap();
        let assets = vec![
            asset("aaaaaaaa1", Some("2020-03-04T05:06:07.000Z"), "a.jpg"),
            ExportAsset { media_type: "video".into(), overlay_path: Some("o.png".into()), ..asset("bbbbbbbb2", None, "b.mp4") },
            asset("cccccccc3", None, "gone.jpg"),
        ];
        let o = opts(&dest);
        let s = run(&assets, &root, &dest, &o);
        assert_eq!((s.total, s.exported, s.missing_source, s.skipped_existing, s.videos_without_overlay), (3, 2, 1, 0, 1));
        assert_eq!(s.bytes_written, 8);
        let f = dest.join("2020").join("03").join("2020-03-04_050607_aaaaaaaa.jpg");
        assert_eq!(fs::read(&f).unwrap(), b"hello");
        assert!(dest.join("undated").join("undated_bbbbbbbb.mp4").is_file());
        let want = parse_iso_to_system_time("2020-03-04T05:06:07.000Z").unwrap();
        let got = fs::metadata(&f).unwrap().modified().unwrap();
        assert_eq!(got.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(), want.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs());
        // no leftover temp files
        assert!(fs::read_dir(f.parent().unwrap()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains("snapvault-part")));
        let s2 = run(&assets, &root, &dest, &o);
        assert_eq!((s2.exported, s2.skipped_existing, s2.missing_source, s2.bytes_written), (0, 2, 1, 0));
    }

    fn make_png(path: &Path, lavfi: &str) -> bool {
        no_window_command(ffmpeg_program("ffmpeg"))
            .args(["-y", "-v", "error", "-f", "lavfi", "-i", lavfi, "-frames:v", "1"])
            .arg(path)
            .status()
            .is_ok_and(|s| s.success())
    }

    #[test]
    fn composite_and_invalid_overlay_fallback() {
        if !ffmpeg_ok() {
            eprintln!("ffmpeg missing, skipping");
            return;
        }
        let root = tmp();
        let dest = tmp().join("out");
        assert!(make_png(&root.join("base.png"), "color=c=blue:s=64x48"));
        assert!(make_png(&root.join("base.jpg"), "color=c=blue:s=64x48"));
        assert!(make_png(&root.join("ov.png"), "color=c=red:s=16x16"));
        fs::write(root.join("bad.png"), b"not a png at all").unwrap();
        let with = |id: &str, base: &str, ov: &str| ExportAsset { overlay_path: Some(ov.into()), ..asset(id, Some("2021-01-02T03:04:05.000Z"), base) };
        let assets = vec![with("11111111", "base.jpg", "ov.png"), with("22222222", "base.png", "ov.png"), with("33333333", "base.jpg", "bad.png")];
        let mut o = opts(&dest);
        o.organize = Organize::Flat;
        let s = run(&assets, &root, &dest, &o);
        assert_eq!((s.composited, s.overlay_failed, s.exported, s.failed), (2, 1, 3, 0), "{s:?}");
        let jpg = dest.join("2021-01-02_030405_11111111.jpg");
        let png = dest.join("2021-01-02_030405_22222222.png");
        assert_eq!(fs::read(&jpg).unwrap()[..2], [0xff, 0xd8]);
        assert_eq!(&fs::read(&png).unwrap()[1..4], b"PNG");
        assert_eq!(fs::read(dest.join("2021-01-02_030405_33333333.jpg")).unwrap(), fs::read(root.join("base.jpg")).unwrap());
        // composite output has the base's full resolution and differs from the plain base
        assert_ne!(fs::read(&jpg).unwrap(), fs::read(root.join("base.jpg")).unwrap());
        // composites are rewritten (never skipped) on re-export
        let s2 = run(&assets[..1], &root, &dest, &o);
        assert_eq!((s2.composited, s2.skipped_existing), (1, 0));
        // overlays off -> plain copy
        o.apply_overlays = false;
        let dest2 = tmp().join("o2");
        o.destination = dest2.display().to_string();
        let s3 = run(&assets[..1], &root, &dest2, &o);
        assert_eq!((s3.composited, s3.overlay_failed, s3.exported), (0, 0, 1));
    }

    #[test]
    fn cancel_stops_and_leaves_no_partial() {
        let root = tmp();
        let dest = tmp().join("out");
        for i in 0..5 {
            fs::write(root.join(format!("f{i}.jpg")), vec![1u8; 3 << 20]).unwrap();
        }
        let assets: Vec<_> = (0..5).map(|i| asset(&format!("id{i}xxxxxx"), None, &format!("f{i}.jpg"))).collect();
        let ctl = ExportCtl::default();
        let o = opts(&dest);
        let s = export_assets(&assets, &root, &dest, &o, &ctl, |processed, _| {
            if processed == 2 {
                ctl.cancel.store(true, Ordering::SeqCst);
            }
        });
        assert!(s.cancelled);
        assert_eq!(s.exported, 2);
        let names: Vec<String> = fs::read_dir(dest.join("undated")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names.iter().all(|n| !n.contains("snapvault-part")));
        // cancel during a copy: partial removed
        let ctl2 = ExportCtl::default();
        ctl2.cancel.store(true, Ordering::SeqCst);
        let t = dest.join("direct.jpg");
        assert!(matches!(copy_streaming(&root.join("f0.jpg"), &t, &ctl2).unwrap().0, Step::Cancelled));
        assert!(!t.exists() && !temp_sibling(&t, false).exists());
    }

    #[test]
    fn cancel_kills_ffmpeg_promptly() {
        if !ffmpeg_ok() {
            return;
        }
        let root = tmp();
        // big enough that the composite takes noticeable time
        assert!(make_png(&root.join("base.png"), "testsrc=s=4000x4000"));
        assert!(make_png(&root.join("ov.png"), "color=c=red:s=64x64"));
        let ctl = Arc::new(ExportCtl::default());
        let c2 = ctl.clone();
        let target = root.join("out.png");
        let h = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            c2.cancel.store(true, Ordering::SeqCst);
        });
        let t = Instant::now();
        let r = composite(&root.join("base.png"), &root.join("ov.png"), &target, &ctl);
        h.join().unwrap();
        // either it was killed, or ffmpeg finished before the cancel landed; never a partial file
        if matches!(r, Ok((Step::Cancelled, _))) {
            assert!(t.elapsed() < Duration::from_secs(5));
            assert!(!target.exists());
        }
        assert!(!temp_sibling(&target, true).exists());
    }

    #[test]
    fn single_flight_guard() {
        let ctl = Arc::new(ExportCtl::default());
        let g = ctl.try_start().unwrap();
        assert!(ctl.try_start().unwrap_err().contains("already running"));
        drop(g);
        assert!(ctl.try_start().is_ok());
    }

    #[test]
    fn year_parsing() {
        assert_eq!(parse_year(None).unwrap(), None);
        assert_eq!(parse_year(Some(" ")).unwrap(), None);
        assert_eq!(parse_year(Some("2020")).unwrap(), Some(2020));
        assert!(parse_year(Some("abc")).is_err());
    }

    // ------------------------------------------------------------ chat export tests

    fn chat_db() -> Mutex<Connection> {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_SQL).unwrap();
        Mutex::new(conn)
    }

    fn add_thread(db: &Mutex<Connection>, id: &str, title: Option<&str>, ext: &str) {
        db.lock().unwrap().execute("INSERT INTO chat_threads (id, external_id, title) VALUES (?1, ?2, ?3)", rusqlite::params![id, ext, title]).unwrap();
    }

    fn add_msg(db: &Mutex<Connection>, id: &str, thread: &str, sender: &str, body: Option<&str>, at: &str, me: bool) {
        db.lock()
            .unwrap()
            .execute(
                "INSERT INTO chat_messages (id, thread_id, sender, body, sent_at, message_type, source, dedupe_key, is_sender)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'TEXT', 'chat_history', ?1, ?6)",
                rusqlite::params![id, thread, sender, body, at, me as i64],
            )
            .unwrap();
    }

    fn add_media(db: &Mutex<Connection>, msg: &str, asset: &str, kind: &str, path: &str) {
        let conn = db.lock().unwrap();
        conn.execute("INSERT OR IGNORE INTO assets (id, source_type, media_type, original_path) VALUES (?1, 'chat', ?2, ?3)", rusqlite::params![asset, kind, path]).unwrap();
        conn.execute("INSERT INTO chat_message_assets (message_id, asset_id) VALUES (?1, ?2)", rusqlite::params![msg, asset]).unwrap();
    }

    fn copts(dest: &Path, fmt: &str) -> ExportChatsOptions {
        serde_json::from_value(serde_json::json!({ "destination": dest.display().to_string(), "format": fmt })).unwrap()
    }

    fn crun(db: &Mutex<Connection>, root: &Path, o: &ExportChatsOptions) -> ChatExportSummary {
        chat_export_checked(db, root, o, &ExportCtl::default(), "2024-01-02T03:04:05Z", &[], &[], |_, _| {}).unwrap()
    }

    fn subdirs(dest: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dest).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn chat_options_defaults_and_sanitizing() {
        let o: ExportChatsOptions = serde_json::from_str(r#"{"destination":"x"}"#).unwrap();
        assert_eq!((o.format, o.include_media, o.thread_id), (ChatFormat::Html, true, None));
        assert!(serde_json::from_str::<ExportChatsOptions>(r#"{"destination":"x","format":"pdf"}"#).is_err());
        for (raw, want) in [("../evil", "_evil"), ("CON", "_CON"), ("com1.txt", "_com1.txt"), ("a:b*c?", "a_b_c_"), ("  ..  ", "chat"), ("trail. .", "trail"), ("", "chat"), ("ok name", "ok name")] {
            assert_eq!(sanitize_component(raw), want, "{raw:?}");
        }
        assert_eq!(sanitize_component(&"x".repeat(200)).len(), 60);
        assert_eq!(sanitize_component("\u{1}\u{2}"), "__");
    }

    #[test]
    fn chat_html_escapes_everything_hostile() {
        let root = tmp();
        let dest = tmp().join("out");
        let db = chat_db();
        add_thread(&db, "th-esc-0001", Some("../CON <b>t</b>"), "e");
        let body = "<script>alert(\"x\")</script> & 'q' <img src=x onerror=1>";
        add_msg(&db, "m1", "th-esc-0001", "<i>evil</i>", Some(body), "2020-01-01T00:00:00.000Z", false);
        fs::write(root.join("a.png"), b"png").unwrap();
        add_media(&db, "m1", "a\"><script>1</script>", "image", "a.png");
        let s = crun(&db, &root, &copts(&dest, "html"));
        assert_eq!((s.threads, s.messages, s.media_copied, s.failed), (1, 1, 1, 0));
        let dirs = subdirs(&dest);
        assert_eq!(dirs.len(), 1);
        assert!(dirs[0].ends_with("-th-esc-0") && !dirs[0].contains(['/', '\\', '<', '>']) && !dirs[0].starts_with('.'), "{dirs:?}");
        let html = fs::read_to_string(dest.join(&dirs[0]).join("chat.html")).unwrap();
        assert!(!html.contains("<script") && !html.contains("<i>") && !html.contains("<b>") && !html.contains("onerror=1>"), "{html}");
        assert!(html.contains("&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; &#39;q&#39;"));
        assert!(html.contains("&lt;i&gt;evil&lt;/i&gt;") && html.contains("&lt;b&gt;t&lt;/b&gt;"));
        assert!(html.contains("<img loading=\"lazy\" src=\"media/a___script_1__script_.png\""), "{html}");
        assert!(!html.contains("http://") && !html.contains("https://"));
        assert!(dest.join(&dirs[0]).join("media").join("a___script_1__script_.png").is_file());
        // reserved name title
        add_thread(&db, "th-con-0002", Some("NUL"), "e2");
        let dest2 = tmp().join("o2");
        let mut o = copts(&dest2, "txt");
        o.thread_id = Some("th-con-0002".into());
        crun(&db, &root, &o);
        assert_eq!(subdirs(&dest2), vec!["_NUL-th-con-0"]);
    }

    #[test]
    fn chat_order_and_empty_row_skipping_match_the_app() {
        let root = tmp();
        let dest = tmp().join("out");
        let db = chat_db();
        add_thread(&db, "th-ord-0001", None, "friend");
        let t = "th-ord-0001";
        add_msg(&db, "m-c", t, "friend", Some("third"), "2020-01-03T00:00:00.000Z", false);
        add_msg(&db, "m-b2", t, "friend", Some("second-b"), "2020-01-02T00:00:00.000Z", true);
        add_msg(&db, "m-b1", t, "friend", Some("second-a"), "2020-01-02T00:00:00.000Z", false);
        add_msg(&db, "m-a", t, "friend", Some("first"), "2020-01-01T00:00:00.000Z", false);
        add_msg(&db, "m-null", t, "friend", None, "2020-01-01T12:00:00.000Z", false);
        add_msg(&db, "m-empty", t, "friend", Some(""), "2020-01-01T13:00:00.000Z", false);
        add_msg(&db, "m-ws", t, "friend", Some("  \n "), "2020-01-01T14:00:00.000Z", false);
        add_msg(&db, "m-media", t, "friend", None, "2020-01-04T00:00:00.000Z", false);
        add_media(&db, "m-media", "as1", "video", "gone.mp4");
        let s = crun(&db, &root, &copts(&dest, "json"));
        assert_eq!((s.messages, s.media_missing, s.media_copied), (5, 1, 0));
        let dir = dest.join(&subdirs(&dest)[0]);
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(dir.join("chat.json")).unwrap()).unwrap();
        let ids: Vec<&str> = v["messages"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["m-a", "m-b1", "m-b2", "m-c", "m-media"]);
        // parity with the app's own paged query
        let conn = db.lock().unwrap();
        let (app, _) = crate::chats::messages_after(&conn, t, None, 100).unwrap();
        assert_eq!(app.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ids);
        assert_eq!(v["thread"], serde_json::json!({"id": t, "title": "friend"}));
        assert_eq!(v["exported_at"], "2024-01-02T03:04:05Z");
        let m = &v["messages"][2];
        assert_eq!((m["sender"].as_str(), m["is_sender"].as_bool(), m["text"].as_str()), (Some("Me"), Some(true), Some("second-b")));
        assert_eq!(v["messages"][1]["sender"], "friend");
        assert_eq!(v["messages"][4]["text"], serde_json::Value::Null);
        assert_eq!(v["messages"][4]["media"], serde_json::json!([]));
    }

    #[test]
    fn chat_media_copy_relative_refs_and_missing() {
        let root = tmp();
        let dest = tmp().join("out");
        let db = chat_db();
        add_thread(&db, "th-med-0001", Some("Pals"), "e");
        let t = "th-med-0001";
        fs::create_dir_all(root.join("chat_media")).unwrap();
        fs::write(root.join("chat_media").join("p.jpg"), b"jpgdata").unwrap();
        fs::write(root.join("chat_media").join("v.mp4"), b"vid").unwrap();
        fs::write(root.join("chat_media").join("d.pdf"), b"pdf").unwrap();
        add_msg(&db, "m1", t, "x", Some("pic"), "2020-01-01T00:00:00.000Z", false);
        add_media(&db, "m1", "asset-p", "image", "chat_media/p.jpg");
        add_msg(&db, "m2", t, "x", None, "2020-01-02T00:00:00.000Z", true);
        add_media(&db, "m2", "asset-p", "image", "chat_media/p.jpg"); // same asset twice: one copy
        add_media(&db, "m2", "asset-v", "video", "chat_media/v.mp4");
        add_media(&db, "m2", "asset-d", "audio", "chat_media/d.pdf");
        add_media(&db, "m2", "asset-x", "image", "chat_media/missing.jpg");
        let s = crun(&db, &root, &copts(&dest, "html"));
        assert_eq!((s.messages, s.media_copied, s.media_missing, s.failed), (2, 3, 1, 0));
        let dir = dest.join(&subdirs(&dest)[0]);
        assert_eq!(fs::read(dir.join("media").join("asset-p.jpg")).unwrap(), b"jpgdata");
        let html = fs::read_to_string(dir.join("chat.html")).unwrap();
        assert!(html.contains("<video controls preload=\"none\" src=\"media/asset-v.mp4\">"));
        assert!(html.contains("<a href=\"media/asset-d.pdf\">"));
        assert!(html.contains("[attachment not included]")); // the missing one
        assert!(!html.contains(&root.display().to_string()));
        assert!(fs::read_dir(dir.join("media")).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains("snapvault-part")));
        let txt_dest = tmp().join("t");
        let s = crun(&db, &root, &copts(&txt_dest, "txt"));
        assert_eq!(s.media_copied, 3);
        let txt = fs::read_to_string(txt_dest.join(&subdirs(&txt_dest)[0]).join("chat.txt")).unwrap();
        assert!(txt.contains("[2020-01-01 00:00:00 UTC] x: pic") && txt.contains("[2020-01-02 00:00:00 UTC] Me: \n"), "{txt}");
        assert!(txt.contains("    [media] media/asset-p.jpg\n") && txt.contains("    [media] media/asset-v.mp4\n"), "{txt}");
        // include_media = false: no copies, no media dir, nothing counted missing
        let d3 = tmp().join("n");
        let mut o = copts(&d3, "json");
        o.include_media = false;
        let s = crun(&db, &root, &o);
        assert_eq!((s.media_copied, s.media_missing), (0, 0));
        assert!(!d3.join(&subdirs(&d3)[0]).join("media").exists());
    }

    #[test]
    fn chat_all_threads_one_folder_each_and_unknown_thread_errors() {
        let root = tmp();
        let dest = tmp().join("out");
        let db = chat_db();
        for (i, title) in [(1, Some("Same")), (2, Some("Same")), (3, None)] {
            let id = format!("th-all-{i}000");
            add_thread(&db, &id, title, &format!("ext{i}"));
            add_msg(&db, &format!("m{i}"), &id, "f", Some("hi"), "2020-01-01T00:00:00.000Z", false);
        }
        add_thread(&db, "th-all-4000", Some("Empty"), "ext4");
        let s = crun(&db, &root, &copts(&dest, "html"));
        assert_eq!((s.threads, s.messages, s.failed, s.cancelled), (4, 3, 0, false));
        let dirs = subdirs(&dest);
        assert_eq!(dirs, vec!["Empty-th-all-4", "Same-th-all-1", "Same-th-all-2", "ext3-th-all-3"]);
        assert!(dirs.iter().all(|d| dest.join(d).join("chat.html").is_file()));
        let e = chat_export_checked(&db, &root, &ExportChatsOptions { thread_id: Some("nope".into()), ..copts(&dest, "html") }, &ExportCtl::default(), "n", &[], &[], |_, _| {});
        assert!(e.unwrap_err().contains("not found"));
    }

    #[test]
    fn chat_cancel_removes_partial_output() {
        let root = tmp();
        let db = chat_db();
        add_thread(&db, "th-can-0001", Some("A"), "e1");
        add_thread(&db, "th-can-0002", Some("B"), "e2");
        {
            let mut conn = db.lock().unwrap();
            let tx = conn.transaction().unwrap();
            for i in 0..2100 {
                tx.execute(
                    "INSERT INTO chat_messages (id, thread_id, sender, body, sent_at, message_type, source, dedupe_key, is_sender) VALUES (?1, 'th-can-0001', 'f', 'x', ?2, 'TEXT', 'chat_history', ?1, 0)",
                    rusqlite::params![format!("m{i:05}"), format!("2020-01-01T00:{:02}:{:02}.{:03}Z", i / 3600 % 60, i / 60 % 60, i % 60)],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }
        // cancel mid-thread (detected at the 2000th message)
        let dest = tmp().join("out");
        let ctl = ExportCtl::default();
        let s = chat_export_checked(&db, &root, &copts(&dest, "html"), &ctl, "n", &[], &[], |p, _| {
            if p == 1100 {
                ctl.cancel.store(true, Ordering::SeqCst);
            }
        })
        .unwrap();
        assert!(s.cancelled);
        assert_eq!((s.threads, s.messages), (0, 0));
        assert!(subdirs(&dest).is_empty(), "{:?}", subdirs(&dest));
        // cancel with a media copy pending: nothing left behind
        let dest2 = tmp().join("o2");
        fs::write(root.join("big.bin"), vec![1u8; 2 << 20]).unwrap();
        add_msg(&db, "mm", "th-can-0002", "f", None, "2020-01-01T00:00:00.000Z", false);
        add_media(&db, "mm", "ab", "audio", "big.bin");
        let ctl2 = ExportCtl::default();
        ctl2.cancel.store(true, Ordering::SeqCst);
        let mut o = copts(&dest2, "html");
        o.thread_id = Some("th-can-0002".into());
        let s = chat_export_checked(&db, &root, &o, &ctl2, "n", &[], &[], |_, _| {}).unwrap();
        assert!(s.cancelled && s.threads == 0 && subdirs(&dest2).is_empty());
        // pre-existing folder survives a cancelled re-export
        crun(&db, &root, &o);
        let dir = dest2.join(&subdirs(&dest2)[0]);
        let before = fs::read_to_string(dir.join("chat.html")).unwrap();
        let ctl3 = ExportCtl::default();
        ctl3.cancel.store(true, Ordering::SeqCst);
        let s = chat_export_checked(&db, &root, &o, &ctl3, "n", &[], &[], |_, _| {}).unwrap();
        assert!(s.cancelled);
        assert_eq!(fs::read_to_string(dir.join("chat.html")).unwrap(), before);
        // a full uncancelled run of the big thread writes everything
        let dest3 = tmp().join("o3");
        let mut o = copts(&dest3, "json");
        o.thread_id = Some("th-can-0001".into());
        let s = crun(&db, &root, &o);
        assert_eq!((s.messages, s.threads), (2100, 1));
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(dest3.join(&subdirs(&dest3)[0]).join("chat.json")).unwrap()).unwrap();
        assert_eq!(v["messages"].as_array().unwrap().len(), 2100);
    }

    #[test]
    fn chat_destination_validation_is_reused() {
        let root = tmp();
        let db = chat_db();
        add_thread(&db, "th-val-0001", None, "e");
        let e = chat_export_checked(&db, &root, &copts(&root.join("sub"), "html"), &ExportCtl::default(), "n", &[], &[], |_, _| {});
        assert!(e.unwrap_err().contains("inside"));
        assert!(!root.join("sub").exists());
        let e = chat_export_checked(&db, &root, &copts(Path::new("relative"), "html"), &ExportCtl::default(), "n", &[], &[], |_, _| {});
        assert!(e.is_err());
        let sys = tmp();
        let e = chat_export_checked(&db, &root, &copts(&sys.join("o"), "html"), &ExportCtl::default(), "n", &[sys.clone()], &[], |_, _| {});
        assert!(e.is_err());
    }

    #[test]
    fn progress_events_carry_kind_and_matching_summary() {
        let ev = |kind, status, summary| serde_json::to_value(ProgressEvent { kind, status, processed: None, total: None, message: None, summary }).unwrap();
        let m = ev("memories", "completed", Some(serde_json::to_value(ExportSummary::default()).unwrap()));
        assert_eq!(m["kind"], "memories");
        assert!(m["summary"].get("exported").is_some());
        let c = ev("chats", "cancelled", Some(serde_json::to_value(ChatExportSummary::default()).unwrap()));
        assert_eq!(c["kind"], "chats");
        assert!(c["summary"].get("media_copied").is_some() && c["summary"].get("exported").is_none());
        assert_eq!(ev("chats", "progress", None).get("summary"), None);
        // one shared guard: while either export holds it, the other cannot start
        let ctl = Arc::new(ExportCtl::default());
        let g = ctl.try_start().unwrap();
        assert!(ctl.try_start().unwrap_err().contains("already running"));
        drop(g);
    }
}

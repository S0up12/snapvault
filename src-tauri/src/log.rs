//! Tiny logger: `log_info!`/`log_warn!`/`log_error!` write `<utc timestamp> LEVEL message`
//! to stderr and to `<app_data>/logs/snapvault.log` (rotated to `snapvault.log.1`
//! at ~2 MB, so at most ~4 MB on disk). No deps.
//!
//! Privacy rule: log ids and counts only - never filesystem paths, filenames,
//! or user content (chat text, tags, locations).
//!
//! Before `init` (and in tests) lines go to stderr only.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_FRONTEND_MSG: usize = 2048;

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

pub struct Sink {
    path: PathBuf,
    file: Option<File>,
    size: u64,
    max: u64,
}

impl Sink {
    pub fn new(path: PathBuf, max: u64) -> Result<Sink, String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("failed to create log dir: {e}"))?;
        }
        let mut sink = Sink { path, file: None, size: 0, max };
        sink.open()?;
        Ok(sink)
    }

    fn open(&mut self) -> Result<(), String> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("failed to open log file: {e}"))?;
        self.size = file.metadata().map(|m| m.len()).unwrap_or(0);
        self.file = Some(file);
        Ok(())
    }

    fn rotated_path(&self) -> PathBuf {
        let mut name = self.path.as_os_str().to_owned();
        name.push(".1");
        PathBuf::from(name)
    }

    pub fn write_line(&mut self, line: &str) {
        let len = line.len() as u64 + 1;
        if self.size > 0 && self.size + len > self.max {
            self.file = None; // close the handle first (required on Windows)
            let rotated = self.rotated_path();
            let _ = fs::remove_file(&rotated);
            let _ = fs::rename(&self.path, &rotated);
            let _ = self.open();
        }
        if self.file.is_none() && self.open().is_err() {
            return;
        }
        if let Some(file) = self.file.as_mut() {
            if writeln!(file, "{line}").is_ok() {
                self.size += len;
            }
        }
    }
}

/// Sets up the file sink. Safe to call once; later calls are ignored.
pub fn init(logs_dir: PathBuf) -> Result<(), String> {
    let sink = Sink::new(logs_dir.join("snapvault.log"), MAX_LOG_BYTES)?;
    let _ = SINK.set(Mutex::new(sink));
    Ok(())
}

/// `YYYY-MM-DDTHH:MM:SSZ` from unix seconds (civil-from-days, no chrono).
fn utc_timestamp(secs: u64) -> String {
    let (days, rem) = ((secs / 86400) as i64, secs % 86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn write(level: &str, msg: &str) {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // One line per record: keep user-visible newlines from forging log lines.
    let msg = msg.replace(['\r', '\n'], " ");
    let line = format!("{} {level:<5} {msg}", utc_timestamp(secs));
    eprintln!("{line}");
    if let Some(sink) = SINK.get() {
        if let Ok(mut s) = sink.lock() {
            s.write_line(&line);
        }
    }
}

/// Truncates to at most `max` bytes on a char boundary.
pub fn truncate(msg: &str, max: usize) -> &str {
    if msg.len() <= max {
        return msg;
    }
    let mut end = max;
    while !msg.is_char_boundary(end) {
        end -= 1;
    }
    &msg[..end]
}

macro_rules! log_info { ($($a:tt)*) => { $crate::log::write("INFO", &format!($($a)*)) }; }
macro_rules! log_warn { ($($a:tt)*) => { $crate::log::write("WARN", &format!($($a)*)) }; }
macro_rules! log_error { ($($a:tt)*) => { $crate::log::write("ERROR", &format!($($a)*)) }; }
pub(crate) use {log_error, log_info, log_warn};

/// Mirrors a frontend console message into the log file. `level`: error|warn|anything else = info.
#[tauri::command]
pub fn log_frontend(level: String, message: String) -> Result<(), String> {
    let msg = format!("[ui] {}", truncate(&message, MAX_FRONTEND_MSG));
    match level.as_str() {
        "error" => log_error!("{msg}"),
        "warn" => log_warn!("{msg}"),
        _ => log_info!("{msg}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_known_values() {
        assert_eq!(utc_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_timestamp(1_000_000_000), "2001-09-09T01:46:40Z");
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate("abc", 10), "abc");
        assert_eq!(truncate("aéb", 2), "a"); // 'é' is 2 bytes, cut at 2 would split it
        let long = "x".repeat(5000);
        assert_eq!(truncate(&long, MAX_FRONTEND_MSG).len(), MAX_FRONTEND_MSG);
    }

    #[test]
    fn rotates_and_keeps_two_files() {
        let dir = std::env::temp_dir().join(format!("snapvault-log-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("snapvault.log");
        let mut sink = Sink::new(path.clone(), 100).unwrap();
        for i in 0..20 {
            sink.write_line(&format!("line number {i:02} padding padding"));
        }
        let rotated = dir.join("snapvault.log.1");
        assert!(rotated.exists());
        assert!(!dir.join("snapvault.log.2").exists());
        assert!(fs::metadata(&path).unwrap().len() <= 100);
        assert!(fs::read_to_string(&path).unwrap().contains("line number 19"));
        let _ = fs::remove_dir_all(&dir);
    }
}

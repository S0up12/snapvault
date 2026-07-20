use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

const SETTINGS_FILE: &str = "performance_settings.json";

/// Maps to ffmpeg's `-preset` for the H.264 transcode in `thumbnails.rs` -
/// the single biggest CPU cost in the app. Faster presets trade a larger
/// output file for much less CPU time per video, which is the actual lever
/// low-end hardware needs (playback quality is unaffected either way).
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscodePreset {
    Fastest,
    Balanced,
    Quality,
}

impl TranscodePreset {
    pub fn ffmpeg_preset(self) -> &'static str {
        match self {
            Self::Fastest => "ultrafast",
            Self::Balanced => "fast",
            Self::Quality => "medium",
        }
    }
}

impl Default for TranscodePreset {
    fn default() -> Self {
        // The hardcoded preset this replaces - existing installs see no
        // behavior change until they opt into something faster.
        Self::Quality
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct PerformanceSettings {
    #[serde(default)]
    pub transcode_preset: TranscodePreset,
    /// Caps ffmpeg's encoder threads at half the machine's cores instead of
    /// all of them, so the rest of the system stays usable while a big
    /// backlog runs in the background. Off by default (matches the prior
    /// unrestricted behavior).
    #[serde(default)]
    pub limit_cpu_usage: bool,
}

impl Default for PerformanceSettings {
    fn default() -> Self {
        Self {
            transcode_preset: TranscodePreset::default(),
            limit_cpu_usage: false,
        }
    }
}

impl PerformanceSettings {
    /// `None` means unrestricted (ffmpeg's own default: use every core).
    pub fn ffmpeg_thread_cap(&self) -> Option<usize> {
        if !self.limit_cpu_usage {
            return None;
        }
        let available = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        Some((available / 2).max(1))
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(crate::storage::fixed_app_data_dir(app)?.join(SETTINGS_FILE))
}

#[tauri::command]
pub fn get_performance_settings(app: AppHandle) -> Result<PerformanceSettings, String> {
    let path = settings_path(&app)?;
    if !path.is_file() {
        return Ok(PerformanceSettings::default());
    }
    let contents = fs::read_to_string(&path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_str(&contents).map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

#[tauri::command]
pub fn set_performance_settings(app: AppHandle, settings: PerformanceSettings) -> Result<(), String> {
    let path = settings_path(&app)?;
    let json = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
    fs::write(&path, json).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

const VIEWER_SETTINGS_FILE: &str = "viewer_settings.json";

fn default_true() -> bool {
    true
}

/// Preferences for the fullscreen media lightbox.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct ViewerSettings {
    /// When on, wheel/trackpad scroll and touch swipes move to the next or
    /// previous asset - the primary way of browsing media. Arrow keys and
    /// the prev/next buttons keep working either way. On by default.
    #[serde(default = "default_true")]
    pub vertical_scroll_navigation: bool,
    /// How long to wait after opening a video/voice message before it starts
    /// playing itself, in milliseconds. 0 means play immediately (prior
    /// behavior). The actual delay presets shown in Settings are a frontend
    /// concern - this just persists whatever number it's given.
    #[serde(default)]
    pub autoplay_delay_ms: u32,
}

impl Default for ViewerSettings {
    fn default() -> Self {
        Self { vertical_scroll_navigation: true, autoplay_delay_ms: 0 }
    }
}

fn viewer_settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(crate::storage::fixed_app_data_dir(app)?.join(VIEWER_SETTINGS_FILE))
}

#[tauri::command]
pub fn get_viewer_settings(app: AppHandle) -> Result<ViewerSettings, String> {
    let path = viewer_settings_path(&app)?;
    if !path.is_file() {
        return Ok(ViewerSettings::default());
    }
    let contents = fs::read_to_string(&path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_str(&contents).map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

#[tauri::command]
pub fn set_viewer_settings(app: AppHandle, settings: ViewerSettings) -> Result<(), String> {
    let path = viewer_settings_path(&app)?;
    let json = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
    fs::write(&path, json).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fastest_and_balanced_map_to_faster_ffmpeg_presets_than_quality() {
        assert_eq!(TranscodePreset::Fastest.ffmpeg_preset(), "ultrafast");
        assert_eq!(TranscodePreset::Balanced.ffmpeg_preset(), "fast");
        assert_eq!(TranscodePreset::Quality.ffmpeg_preset(), "medium");
    }

    #[test]
    fn thread_cap_is_none_when_limit_disabled() {
        let settings = PerformanceSettings { transcode_preset: TranscodePreset::Quality, limit_cpu_usage: false };
        assert_eq!(settings.ffmpeg_thread_cap(), None);
    }

    #[test]
    fn thread_cap_is_at_least_one_when_limit_enabled() {
        let settings = PerformanceSettings { transcode_preset: TranscodePreset::Quality, limit_cpu_usage: true };
        assert!(settings.ffmpeg_thread_cap().unwrap() >= 1);
    }

    #[test]
    fn vertical_scroll_navigation_defaults_to_on() {
        assert!(ViewerSettings::default().vertical_scroll_navigation);
    }

    #[test]
    fn missing_field_in_old_settings_file_deserializes_to_default_on() {
        let settings: ViewerSettings = serde_json::from_str("{}").unwrap();
        assert!(settings.vertical_scroll_navigation);
        assert_eq!(settings.autoplay_delay_ms, 0);
    }
}

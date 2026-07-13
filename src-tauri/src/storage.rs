use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::db::DbState;

const POINTER_FILE: &str = "media_location.json";
const MEDIA_DIR_NAMES: [&str; 3] = ["imports", "thumbnails", "playback"];

#[derive(Serialize, Deserialize)]
struct PointerFile {
    media_root: String,
}

struct Resolution {
    media_root: PathBuf,
    pointer_confirmed: bool,
}

#[derive(Clone, Serialize)]
pub struct StorageInfo {
    pub media_root: String,
    pub is_default: bool,
    pub needs_first_run_choice: bool,
}

fn pointer_path(fixed_dir: &Path) -> PathBuf {
    fixed_dir.join(POINTER_FILE)
}

/// Whether `imports`/`thumbnails`/`playback` already exist under `fixed_dir` -
/// the signal that this is an install from before the media root became
/// configurable, so its data must be silently adopted in place rather than
/// treated as a fresh install needing a first-run prompt.
fn legacy_dirs_present(fixed_dir: &Path) -> bool {
    MEDIA_DIR_NAMES.iter().any(|name| fixed_dir.join(name).is_dir())
}

fn read_pointer(fixed_dir: &Path) -> Result<Option<PathBuf>, String> {
    let path = pointer_path(fixed_dir);
    if !path.is_file() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let parsed: PointerFile = serde_json::from_str(&contents)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
    Ok(Some(PathBuf::from(parsed.media_root)))
}

fn write_pointer(fixed_dir: &Path, media_root: &Path) -> Result<(), String> {
    let path = pointer_path(fixed_dir);
    let payload = PointerFile {
        media_root: media_root.display().to_string(),
    };
    let json = serde_json::to_string_pretty(&payload).map_err(|err| err.to_string())?;
    fs::write(&path, json).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

/// Resolves the media root for `fixed_dir` (the OS-default, never-configurable
/// app-data dir metadata.db always lives in - see module docs on
/// `resolve_media_root`). Adopts `fixed_dir` itself as the confirmed root the
/// first time it's called against an install that already has media there
/// from before this was configurable, so existing libraries are never
/// silently relocated or re-prompted for a choice they already made
/// implicitly by using the app.
fn resolve_at(fixed_dir: &Path) -> Result<Resolution, String> {
    if let Some(media_root) = read_pointer(fixed_dir)? {
        fs::create_dir_all(&media_root)
            .map_err(|err| format!("failed to create media root {}: {err}", media_root.display()))?;
        return Ok(Resolution {
            media_root,
            pointer_confirmed: true,
        });
    }

    if legacy_dirs_present(fixed_dir) {
        write_pointer(fixed_dir, fixed_dir)?;
        return Ok(Resolution {
            media_root: fixed_dir.to_path_buf(),
            pointer_confirmed: true,
        });
    }

    // Genuinely fresh install: nothing imported yet, no choice made yet.
    // `fixed_dir` is a safe default for anything that must resolve a path
    // before the frontend's first-run prompt runs, but `pointer_confirmed`
    // stays false so the frontend knows to ask.
    Ok(Resolution {
        media_root: fixed_dir.to_path_buf(),
        pointer_confirmed: false,
    })
}

fn set_media_root_at(fixed_dir: &Path, chosen: Option<&Path>) -> Result<PathBuf, String> {
    let media_root = chosen.map(Path::to_path_buf).unwrap_or_else(|| fixed_dir.to_path_buf());
    fs::create_dir_all(&media_root)
        .map_err(|err| format!("failed to create {}: {err}", media_root.display()))?;
    write_pointer(fixed_dir, &media_root)?;
    Ok(media_root)
}

fn fixed_app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("failed to resolve app data dir: {err}"))?;
    fs::create_dir_all(&dir).map_err(|err| format!("failed to create app data dir {}: {err}", dir.display()))?;
    Ok(dir)
}

/// Resolves where `imports`/`thumbnails`/`playback` live. Independent of
/// `metadata.db`'s location, which always stays at the fixed OS app-data dir
/// (see `db::init`) - splitting the two avoids a chicken-and-egg bootstrap
/// problem (a config file naming the DB's location would itself need a
/// location) and means changing this never requires reconnecting the
/// long-lived `Mutex<Connection>` held in `DbState` for the app's lifetime.
pub fn resolve_media_root(app: &AppHandle) -> Result<PathBuf, String> {
    let fixed_dir = fixed_app_data_dir(app)?;
    Ok(resolve_at(&fixed_dir)?.media_root)
}

#[tauri::command]
pub fn get_storage_info(app: AppHandle) -> Result<StorageInfo, String> {
    let fixed_dir = fixed_app_data_dir(&app)?;
    let resolution = resolve_at(&fixed_dir)?;
    Ok(StorageInfo {
        is_default: resolution.media_root == fixed_dir,
        needs_first_run_choice: !resolution.pointer_confirmed,
        media_root: resolution.media_root.display().to_string(),
    })
}

/// Changes the media root - only when the library is empty. Enforced here,
/// not just via the frontend disabling the control: an existing library's
/// assets carry absolute paths under the old root, and nothing here moves
/// files, so relocating with data present would silently orphan it.
#[tauri::command]
pub fn set_media_root(app: AppHandle, state: tauri::State<DbState>, path: Option<String>) -> Result<String, String> {
    let conn = state.0.lock().map_err(|err| err.to_string())?;
    let asset_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
        .map_err(|err| err.to_string())?;
    if asset_count > 0 {
        return Err("Reset the library before changing the storage location.".to_string());
    }
    drop(conn);

    let fixed_dir = fixed_app_data_dir(&app)?;
    let chosen = path.map(PathBuf::from);
    let media_root = set_media_root_at(&fixed_dir, chosen.as_deref())?;
    widen_asset_protocol_scope(&app, &media_root)?;
    Ok(media_root.display().to_string())
}

/// Extends the (in-memory, never persisted across restarts) asset-protocol
/// scope to cover a possibly-custom media root's subfolders, alongside the
/// static `$APPDATA/...` entries in `tauri.conf.json` that already cover the
/// default case. Called both at startup (`lib.rs`'s `.setup()`) and right
/// after `set_media_root` succeeds, so a location change takes effect
/// immediately without requiring an app restart.
pub fn widen_asset_protocol_scope(app: &AppHandle, media_root: &Path) -> Result<(), String> {
    let scope = app.asset_protocol_scope();
    for dir_name in MEDIA_DIR_NAMES {
        scope
            .allow_directory(media_root.join(dir_name), true)
            .map_err(|err| format!("failed to widen asset scope for {dir_name}: {err}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("snapvault-storage-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn fresh_install_has_no_confirmed_pointer_and_defaults_to_fixed_dir() {
        let fixed_dir = tempdir();
        let resolution = resolve_at(&fixed_dir).unwrap();
        assert_eq!(resolution.media_root, fixed_dir);
        assert!(!resolution.pointer_confirmed);
    }

    #[test]
    fn legacy_install_auto_adopts_fixed_dir_and_writes_pointer() {
        let fixed_dir = tempdir();
        fs::create_dir_all(fixed_dir.join("imports")).unwrap();

        let resolution = resolve_at(&fixed_dir).unwrap();
        assert_eq!(resolution.media_root, fixed_dir);
        assert!(resolution.pointer_confirmed);
        assert!(pointer_path(&fixed_dir).is_file());

        // Second call is stable - reads the pointer rather than re-deriving
        // from legacy_dirs_present every time.
        let second = resolve_at(&fixed_dir).unwrap();
        assert_eq!(second.media_root, fixed_dir);
        assert!(second.pointer_confirmed);
    }

    #[test]
    fn pointer_file_present_wins_over_legacy_dir_detection() {
        let fixed_dir = tempdir();
        let custom_root = tempdir();
        write_pointer(&fixed_dir, &custom_root).unwrap();

        let resolution = resolve_at(&fixed_dir).unwrap();
        assert_eq!(resolution.media_root, custom_root);
        assert!(resolution.pointer_confirmed);
    }

    #[test]
    fn pointer_file_recreates_a_missing_media_root_directory() {
        let fixed_dir = tempdir();
        let custom_root = tempdir().join("nested").join("does-not-exist-yet");
        write_pointer(&fixed_dir, &custom_root).unwrap();

        let resolution = resolve_at(&fixed_dir).unwrap();
        assert_eq!(resolution.media_root, custom_root);
        assert!(custom_root.is_dir());
    }

    #[test]
    fn set_media_root_at_writes_a_chosen_path() {
        let fixed_dir = tempdir();
        let chosen = tempdir().join("chosen");

        let result = set_media_root_at(&fixed_dir, Some(&chosen)).unwrap();
        assert_eq!(result, chosen);
        assert!(chosen.is_dir());
        assert_eq!(read_pointer(&fixed_dir).unwrap(), Some(chosen));
    }

    #[test]
    fn set_media_root_at_none_reverts_to_fixed_dir() {
        let fixed_dir = tempdir();
        let chosen = tempdir().join("chosen");
        set_media_root_at(&fixed_dir, Some(&chosen)).unwrap();

        let result = set_media_root_at(&fixed_dir, None).unwrap();
        assert_eq!(result, fixed_dir);
        assert_eq!(read_pointer(&fixed_dir).unwrap(), Some(fixed_dir.clone()));
    }

    #[test]
    fn legacy_dirs_present_detects_any_of_the_three_names() {
        let fixed_dir = tempdir();
        assert!(!legacy_dirs_present(&fixed_dir));

        fs::create_dir_all(fixed_dir.join("thumbnails")).unwrap();
        assert!(legacy_dirs_present(&fixed_dir));
    }
}

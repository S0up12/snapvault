# Architecture (verify against code before relying on it)

Stack: Tauri v2, Rust backend (`src-tauri/src`), React+TS+Tailwind frontend (`src`), SQLite via rusqlite (bundled), ffmpeg/ffprobe on PATH for media. Dark "Nocturne" theme, top nav.

## Backend modules (`src-tauri/src`)
- `lib.rs` - app setup, `generate_handler!` registry (all commands listed here)
- `db/` - `mod.rs` (DbState, versioned `MIGRATIONS` via `PRAGMA user_version`; to change schema append a fn to the end of the list, never edit `schema.rs`, which is the frozen v1 baseline), `schema.rs` (tables below)
- `log.rs` - `log::log_info!/log_warn!/log_error!` -> stderr + `app_data/logs/snapvault.log` (2MB x2 rotation); `log_frontend(level, message)` command. Log ids/counts only, never paths or user content.
- `ingestion/` - `mod.rs` (`run_ingestion`, zip extract, `ingestion://progress`), `memories.rs`, `chats.rs`, `profile.rs`
- `thumbnails.rs` - ffmpeg thumbnails (360px webp), HEVC->h264 playback copies, overlay compositing, `thumbnails://progress`, media-processing slot lock
- `library.rs` - memory queries (assets, tags, years, year offset, favorites)
- `chats.rs`, `profile.rs`, `maintenance.rs` (stats/verify/reset), `settings.rs` (performance + viewer), `storage.rs` (media root)

## Commands
run_ingestion, generate_thumbnails, list_memory_assets, list_memory_tags, list_memory_years, memory_year_offset, set_asset_favorite, set_asset_tags, list_chat_threads, list_chat_messages, get_profile_snapshot, get_library_stats, verify_library, reset_library, get_storage_info, set_media_root, get/set_performance_settings, get/set_viewer_settings, get_media_activity, get_media_state, pause_media_processing, resume_media_processing, cancel_media_processing, detect_hw_encoders(forceRefresh?), ensure_playback(assetId), log_frontend (+ greet, count_assets, db_table_names scaffolding)

## DB tables
assets, ingestion_jobs, profile_snapshots, chat_threads, chat_messages, chat_message_assets, memory_collections, memory_items, story_collections, story_items

## Frontend (`src`)
- `views/` Dashboard, Memories, Chats, Stories, Profile, Settings
- `components/` ImportFlow, Lightbox, VirtualTimelineGrid, ThumbnailGenerator, NavBar, TitleBar, StorageChoiceModal, `memories/` (TimelineTile, TagEditorModal, MemoryTimelineScrubber)
- `hooks/` one per backend area (useMemories, useChats, useProfile, useLibraryStats, usePerformanceSettings, useStorageSettings, useViewerSettings)

## Dev
`npm run tauri dev` (Vite on :1420). Windows helper scripts in `scripts/`. Asset protocol scope: `$APPDATA/{thumbnails,playback,imports}`.

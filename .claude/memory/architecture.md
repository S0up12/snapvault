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
run_ingestion, generate_thumbnails, list_memory_assets, list_memory_tags, list_memory_years, memory_year_offset, set_asset_favorite, set_asset_tags, list_chat_threads, list_chat_messages, search_chat_messages, get_profile_snapshot, get_library_stats, verify_library, reset_library, get_storage_info, set_media_root, get/set_performance_settings, get/set_viewer_settings, get_media_activity, get_media_state, pause_media_processing, resume_media_processing, cancel_media_processing, export_memories, cancel_export, get_export_state, detect_hw_encoders(forceRefresh?), ensure_playback(assetId), log_frontend (+ greet, count_assets, db_table_names scaffolding)

## DB tables
(+ FTS5 external-content `chat_messages_fts` over chat_messages.body, migration v4; shadow tables `chat_messages_fts_*`, so filter `chat_messages_fts%` when counting tables)
assets, ingestion_jobs, profile_snapshots, chat_threads, chat_messages, chat_message_assets, memory_collections, memory_items, story_collections, story_items

## Frontend (`src`)
- `views/` Dashboard, Memories, Chats, Stories, Profile, Settings
- `components/` ImportFlow, Lightbox, VirtualTimelineGrid, ThumbnailGenerator, NavBar, TitleBar, StorageChoiceModal, `memories/` (TimelineTile, TagEditorModal, MemoryTimelineScrubber)
- `hooks/` one per backend area (useMemories, useChats, useProfile, useLibraryStats, usePerformanceSettings, useStorageSettings, useViewerSettings)

## Dev
`npm run tauri dev` (Vite on :1420). Windows helper scripts in `scripts/`. Asset protocol scope: `$APPDATA/{thumbnails,playback,imports}`.

## Chat search / jump JSON (serialized snake_case; invoke args camelCase)
`search_chat_messages(query, threadId?, limit? (50, max 200), offset?)` ->
`{"hits":[{"message_id","thread_id","thread_title":string|null,"sent_at","is_sender":bool,"sender_label":"Me"|name,"snippet":"text with match markers"}],"total":int (capped 1000),"total_capped":bool,"has_more":bool}`
Empty/unsearchable query -> empty page. Last token is prefix-matched; AND semantics; emoji/punctuation-only tokens are dropped (unicode61 can't index them).

`list_chat_messages(threadId, before?, limit?, around?, after?)` -> `{"messages":[ChatMessageView],"has_more":bool,"next_before":string|null,"has_more_before":bool,"has_more_after":bool,"next_after":string|null}`.
Cursors are opaque "<sent_at>|<id>". `around` (message id, default limit 60 = 30 before + target + 29 after; window is shorter at thread edges) errors "message not found in this thread" for a foreign/unknown id; it wins over before/after. `after` returns the next `limit` newer messages (chronological). `has_more` == `has_more_before`. Newest page: has_more_after=false, next_after=null; a `before` page has has_more_after=true with next_after = its newest row.

## Memories export (`export.rs`; invoke args camelCase, payloads snake_case)
`export_memories(options)` (async, spawn_blocking) options: `{"destination":string,"filter"?:"all"|"photo"|"video"|"favorite","tag"?:string,"year"?:"2020","apply_overlays"?:true,"organize"?:"flat"|"year"|"year_month"(default),"set_file_times"?:true}` (invoke arg is `{options:{...}}`; inner keys stay snake_case). Same selection as `list_memory_assets` via `library::memory_where`/`FilterParams`; unknown filter = all (the grid value is `favorite`, not `favorites`).
Returns `ExportSummary` `{"total","exported"(incl. composited),"composited","overlay_failed","skipped_existing","missing_source","failed","videos_without_overlay","cancelled":bool,"bytes_written","destination"}`. Err if already running / bad destination / bad year.
`cancel_export()` Err("No export is running.") when idle; `get_export_state()` -> bool.
Event `export://progress`: `{"status":"progress","processed","total","message"}` (<=10/s), then `{"status":"completed"|"cancelled","summary":ExportSummary}` or `{"status":"error","message"}`.
Layout `<dest>/[YYYY/[MM/]]YYYY-MM-DD_HHMMSS_<id8>.<ext>`; NULL/invalid date -> `undated/undated_<id8>.<ext>`; in-run collisions get `_2`... Composited images are `.jpg` (`.png` if base png). Written via `*.snapvault-part*` temp then rename.

## Chats export (`export.rs`, same guard/cancel/event as memories)
`export_chats(options)` (async, spawn_blocking; invoke arg `{options:{...}}`) options: `{"destination":string,"thread_id"?:string (omit = all threads),"format"?:"html"(default)|"txt"|"json","include_media"?:true}`. Same destination validation as memories (`validate_destination`). Err: already running / bad destination / "Chat thread not found.".
Returns `ChatExportSummary` `{"threads","messages","media_copied","media_missing","failed","cancelled":bool,"destination"}`.
ALL `export://progress` events now carry `"kind":"memories"|"chats"`; `completed`/`cancelled` carry that kind's summary (`summary` is the matching shape). Chat progress: `processed`/`total` = messages written / raw message rows in scope (total is an upper bound; the empty-row filter can make processed end lower).
Layout `<dest>/<sanitized title or external_id>-<id8>/chat.{html,txt,json}` + `media/<asset_id>.<ext>` (original file, referenced as `media/<name>`, shared assets copied once per thread). JSON: `{"thread":{"id","title"},"exported_at":"...Z","messages":[{"id","sent_at","sender":"Me"|name,"is_sender","text":string|null,"media":["media/.."]}]}`. Messages come from `chats::messages_after` (same rows/order/filter as `list_chat_messages`), DB lock held per 500-row page only.

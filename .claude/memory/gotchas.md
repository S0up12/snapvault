# Gotchas

- Commands compile but are unreachable unless added to `generate_handler!` in `lib.rs`. **Apply:** every new command.
- Tauri maps Rust `snake_case` args to JS `camelCase`. **Apply:** invoke bindings.
- On Windows, spawn ffmpeg/ffprobe only via `no_window_command` (CREATE_NO_WINDOW) or a console window flashes per spawn.
- ffmpeg picks the output muxer from the extension: temp files must end `.mp4`/`.webp`, not `.x.tmp`.
- Only h264 + aac/mp3 in .mp4/.m4v is treated as playback-safe in WebView2; everything else (HEVC, .mov) is transcoded.
- Observed 2026-10-21: during a dev run, overlay compositing logged `Decode error rate 1 exceeds maximum` / `Cannot determine format of input` for a PNG overlay and the webp output was not written. Likely a corrupt/zero-byte or non-PNG overlay file. **Apply:** thumbnails.rs overlay step should validate the overlay and fall back to the base image instead of failing. Unconfirmed root cause.
- `.claude/` is partly committed (agents, skills, memory); `settings.local.json` stays ignored.

- Overlay PNGs can be truncated/non-PNG (ffmpeg "Cannot determine format of input" / "Decode error rate 1"): thumbnails validate magic bytes + size and retry without overlay on ffmpeg failure. `scale2ref` needs `rw:rh` (not `iw:ih`) to size overlay to the base.
- Failed thumbnails set `assets.thumb_failed=1` so startup resume skips them; manual `generate_thumbnails` clears the flag and retries.
- Memories JSON has no filename; pairing is Nth-of-day vs files sorted by (mtime, stem). Legacy repair script keyed on zip member path + zip mtime instead. Days where JSON/file counts differ are NOT paired (logged, day-level date fallback). Dates are range-checked (year 2010-2100).
- Each import extracts to a fresh imports/<job>/, so `assets.original_path` differs per job. Dedupe keys: memories `external_id='memories:<stem>'`, chats `external_id='chat:<filename>'` (legacy chat rows have NULL external_id -> path fallback only; one-time duplicate possible). A re-import still leaves an unused extracted dir on disk (not cleaned).
- Extraction skips (counts as skipped) entries with backslash/drive letter/reserved Windows names/symlinks and duplicate names (create_new, first wins); caps 500k entries / 512 GiB real bytes; failed extraction deletes imports/<job>.
- In-place HEVC transcode: ensure_playback_copy no longer deletes the old original on extension change; the caller removes it after the DB row is updated. Same-extension case is an atomic rename over the original (original HEVC is intentionally not kept).
- reset_library/set_media_root take MediaSlot but ingestion only holds it in its media phase, not during zip extraction.
- src/index.css @imports Google Fonts (remote); the prod CSP (font-src/style-src self) blocks it, so fonts must be bundled locally. opener scope is https/http `*` because Chats.tsx opens arbitrary chat URLs.
- Logging macros live in `crate::log` (`crate::log::log_info!` etc.); they are not exported at crate root. Never log paths/filenames/user content. Migrations: SCHEMA_SQL is frozen v1; fresh DBs replay later migrations on top of it.
- Asset path columns (original/thumbnail/playback/overlay_path) are stored media-root-relative with `/` (migration v2; legacy absolute rows still work). Use `storage::to_relative` on write and `to_absolute` on read; commands handed to the frontend resolve back to absolute. Ingestion derives the root from `<root>/imports/<job>` (`ingestion::media_root_of`). Migrations now take `(&Connection, &Path media_root)`.
- Encoders can be listed by `ffmpeg -encoders` without a GPU (hw `available` != `works`); only a real test encode proves it. Background `Ok(None)` (already-safe mp4) leaves playback_path NULL, so such assets re-enter load_pending_assets each run (pre-existing; ensure_playback also doesn't write it).
- ffmpeg/ffprobe resolve via `thumbnails::ffmpeg_program` (exe-dir sidecar first, else PATH). externalBin lives only in `src-tauri/tauri.release.conf.json` (used by `npm run release:build`; needs `scripts/Fetch-Ffmpeg.ps1` first) because tauri-build fails on missing sidecar files.
- `assets.playback_failed` (v3) = definitive ffprobe/ffmpeg rejection; excluded from pending, cleared by manual Reprocess. Safe originals now record `playback_path = original`. INFLIGHT is process-global keyed by asset id: parallel tests must use distinct ids.

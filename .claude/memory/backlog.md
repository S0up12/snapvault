# Backlog (updated 2026-10-21)

## Features / larger work
- [ ] Bundle ffmpeg/ffprobe as a Tauri sidecar (today: expected on PATH; Dashboard warns via check_ffmpeg).
- [ ] Stories: tables exist, nothing writes them, `snap_history.json` is skipped; nav item hidden (route still in App.tsx).
- [ ] Chat overlays are never composited; video+overlay compositing missing.
- [ ] Cancel/pause/resume for import and thumbnail runs; `ingestion_jobs` is never written.
- [ ] Typed/streaming chat JSON parsing (Value tree is ~4.4x file size; see decisions.md).
- [ ] Generate TS types from Rust (ts-rs/specta) instead of hand-mirroring.

## Known gaps / small fixes
- [ ] Skipped unsafe zip entries are only logged; surface the count in the import UI.
- [ ] Re-import leaves the freshly extracted `imports/<job>` dir as an orphan.
- [ ] Existing chat rows have NULL `external_id`: first re-import after upgrade duplicates them once.
- [ ] Reset during zip extraction is not blocked (ingestion holds the media slot only in its media phase).
- [ ] Timestamp pairing is Nth-of-day; check whether the JSON Download Link has a media id to match on.
- [ ] Memories tile: favorite/tag strip no longer passes clicks through to the tile.
- [ ] Dialogs: focus return to the opener after Escape not confirmed.
- [ ] `openUrl` permission is any http(s) (chat links need it); consider confirm-before-open.
- [ ] Windows prefix match in `to_relative` may be case-sensitive.
- [ ] One corrupt export video (moov atom missing) keeps logging a transcode error; mark it unplayable.
- [ ] No frontend test runner; add Vitest + a few hook/component tests.
- [ ] Unverified: CSP effect on photo lightbox, dialogs, GitHub link.

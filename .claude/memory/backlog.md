# Backlog (pruned 2026-10-22)

Legend: **[1.0]** = must be done before the first full release. Everything else can ship after 1.0.

## Release gates (not code)
- [ ] **[1.0]** 2-3 people other than the author import their own exports without problems (different sizes, split zips, missing JSON).
- [ ] **[1.0]** Clean-machine install: fresh Windows profile or VM; install, import, close, reopen, upgrade from the previous beta (also covers the 'SnapVault' -> 'Snapvault' productName change; Windows is case-insensitive so it should upgrade in place - unverified).
- [ ] **[1.0]** Decide on code signing (unsigned installer shows a SmartScreen warning).
- [ ] No known data-loss or corruption bugs open for a week of use.

## Correctness / data safety
- [ ] **[1.0]** Timestamp pairing: real exports have EMPTY download links (no `mid`), so pairing is by kind then day order. Kind-aware pairing + unpaired fallback verified live; remaining ambiguity = several same-kind items in one day (cannot be solved without a key from Snapchat; accept and document).
- [x] **[1.0]** Re-import leaves the freshly extracted `imports/<job>` as an orphan (GBs on large exports); prune unreferenced files after ingestion.
- [x] **[1.0]** Existing chat rows have NULL `external_id`: first re-import after upgrade duplicates them once (needs a backfill migration).
- [x] **[1.0]** Reset / media-root move blocked during an import (IngestionGuard, 2026-10-22).
- [x] **[1.0]** Skipped unsafe zip entries are shown in the import UI (verified live 2026-10-22).
- [x] **[1.0]** Auto hardware acceleration benchmarked 2026-10-22 on a 1080x1920 BT.709 HEVC clip: AMF qp retuned 23 -> 28 (size/quality parity with x264); no speed gain vs unrestricted x264 on a 20-thread CPU, ~20% vs the half-thread limit-CPU mode, main win is CPU offload. NVENC/QSV args unmeasured (no hardware).
- [x] **[1.0]** Dialog focus returns to the opener after Escape (verified live for the Lightbox tile and the Export button; virtualized tiles that unmount fall back to body).

## Features (post-1.0)
- [ ] Stories: tables exist, nothing writes them, `snap_history.json` is skipped; nav item hidden (route still in App.tsx).
- [ ] Overlays are only composited for thumbnails and photo exports: not in the full-size viewer, chat media or exported videos.
- [ ] Cancel for an import's extraction phase; `ingestion_jobs` is never written (no import history).
- [ ] Auto-updater (needs signing keys); bundle first, updater right after 1.0.
- [ ] Other export/library ideas: on-this-day, stats, duplicate finder (checksum column unused), remove-from-library, places view, app lock.

## Quality / tech debt (post-1.0)
- [ ] Typed/streaming chat JSON parsing (Value tree is ~4.4x file size; see decisions.md).
- [ ] Generate TS types from Rust (ts-rs/specta) instead of hand-mirroring.
- [ ] No frontend test runner; add Vitest + a few hook/component tests.
- [ ] `openUrl` permission is any http(s) (chat links need it); consider confirm-before-open.
- [ ] Emoji-only chat searches return nothing (FTS tokenizer limitation).
- [ ] Unverified live: photo lightbox under the CSP, native dialogs, the GitHub link.
- [ ] Published release titles/notes still say 'SnapVault'.

## Done (kept short for reference)
Bundled ffmpeg sidecar, pause/resume/cancel for media processing, hardware-accelerated + on-open transcode, corrupt videos marked once, accurate playback stats, record playable originals' playback_path, media_cancelled note, tile click strip fix, case-insensitive path checks, chat search, export (memories + chats), CI/release workflows, relative media paths, versioned migrations, logging with redaction.

---
name: sv-parity
description: Map from SnapVault Rust ingestion features to the legacy SnapCapsule-Web-V2 Python source of truth. Use when fixing timestamps, overlays, thumbnails, chat links, or any Snapchat export quirk.
---
# Legacy parity (read-only reference)

Root: `C:\Users\Sammy\Documents\GitHub\SnapCapsule-Web-V2` (do not modify; never ask for the path).

| Concern | Legacy file | SnapVault target |
|---|---|---|
| Timestamp repair | `scripts/repair_memory_timestamps_from_archives.py` | `src-tauri/src/ingestion/memories.rs` |
| Overlay compositing | `scripts/repair_memory_overlays.py` | `src-tauri/src/thumbnails.rs` |
| Plain thumbnails | `scripts/backfill_plain_thumbnails.py` | `src-tauri/src/thumbnails.rs` |
| Chat media links | `scripts/rebuild_chat_links.py` | `src-tauri/src/ingestion/chats.rs` |
| Audio misclassified as media | `scripts/reclassify_audio_assets.py` | `src-tauri/src/ingestion/` |
| Services / models | `packages/snapcapsule_core/services/`, `models/` | `src-tauri/src/db/schema.rs` |

Method: `Grep` the legacy file for the specific function first; read only that function. Port the behavior, not the Python structure. Note any behavioral difference you intentionally keep in `.claude/memory/decisions.md`.

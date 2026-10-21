# Product

<!-- impeccable:product-schema 1 -->

## Platform

desktop

## Users

People who've exported their own data via Snapchat's "Download My Data" tool and want to actually browse it — the raw export is a pile of loose media files and JSON, not something you can look through. SnapVault is distributed for other Snapchat users to self-host on their own machine, not just for personal use: each person installs it, imports their own export, and everything stays local to their device.

## Product Purpose

View and interact with an exported Snapchat archive — memories (photos/videos), chat history, and profile data — as a coherent, browsable library instead of a raw file dump. Recreates each snap as it originally looked (compositing caption/drawing overlays back onto the base media) and repairs the timestamps Snapchat's export leaves broken or missing.

## Positioning

A full rewrite of an earlier Dockerized web app (SnapCapsule) that required standing up Docker, PostgreSQL, Redis, and Celery just to view your own export. SnapVault is a single lightweight desktop installer (Tauri) with no server infrastructure at all — everything (extraction, database, media processing) runs locally in the user's own OS AppData directory. That's the meaningful difference a competing viewer built as a hosted/server-based tool couldn't claim: nothing about using it ever leaves the user's machine, and there's nothing to deploy.

## Operating Context

Typical flow: user downloads their Snapchat data export (.zip) from Snapchat, imports it into SnapVault, which streams extraction/parsing progress via Tauri IPC while it derives absolute-truth timestamps and tags from `memories_history.json` and related export JSON (export EXIF data is not trusted), composites overlay images onto their base media, and generates thumbnails/transcodes video via bundled ffmpeg. The user then browses via Dashboard, Memories (grid with filters, tags, favorites, year scrubber), Chats, Stories, and Profile views, and manages storage/performance/viewer preferences in Settings.

## Capabilities and Constraints

- Rust backend does all CPU-intensive work (zip extraction, JSON parsing, media processing) on background threads; the React UI never blocks.
- SQLite is the only datastore — no external database.
- Bundled ffmpeg handles overlay compositing, thumbnail generation, and video transcoding (including in-place HEVC transcode).
- Must handle large archives efficiently (low memory footprint) and stream progress/status to the UI during import.
- Configurable performance settings so the app stays usable on low-end hardware (transcode presets, grid density, etc.), and a configurable media storage location.
- Cross-platform file paths — developed on Windows first, but must not hardcode path separators.
- Reference architecture for porting ingestion/edge-case logic (timestamp repair, overlay compositing, JSON models) lives in a sibling repo, `SnapCapsule-Web-V2` (Python/FastAPI-era codebase) — not shipped, but the source of truth for quirks in Snapchat's export format.

## Brand Commitments

Product name is **SnapVault** (rebranded from the earlier SnapCapsule project this rewrites). Current UI direction is a dark "Nocturne" theme with a top nav bar (see `redesign/SnapVault Redesign.dc.html` for the design reference used for the latest pass) — full visual system belongs in DESIGN.md, not here.

## Evidence on Hand

No user testimonials, benchmarks, or case studies — none should be fabricated. The only concrete reference asset is the SnapCapsule-Web-V2 codebase mentioned above, used for logic/behavior parity, not for visual design.

## Product Principles

1. No server infrastructure, ever — no Docker, Postgres, Redis, or Celery. Everything runs locally.
2. Export JSON is the source of truth for timestamps/location/tags — never trust media file EXIF.
3. Accurately reconstruct the original snap (overlay compositing) rather than showing raw base + overlay separately.
4. Stay lightweight and installable by an average desktop user, including low-end hardware.
5. Privacy by construction: imported data and generated media never leave the user's device.

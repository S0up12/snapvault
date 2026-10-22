# Changelog

All notable changes to SnapVault. Format: [Keep a Changelog](https://keepachangelog.com/).

## [0.1.0-beta.2] - 2026-10-22

### Added
- Export: memories to a folder (overlays composited onto photos, original dates on the files, organized by year/month) and chats as HTML, text or JSON, with cancel and a global progress strip.
- Chat search: full-text search (SQLite FTS5) across all messages with highlighted snippets, optional per-chat scope, and jump-to-message with loading in both directions. Shortcuts: `/` and Ctrl+K.

### Changed
- Hardware-accelerated video conversion is now on "Auto" by default (falls back to the CPU encoder when no GPU encoder works). Existing saved settings are kept.

### Fixed
- Folder checks for the media location and exports now ignore letter case on Windows, so system folders are rejected however the path is typed.

## [0.1.0-beta.1] - 2026-10-21

First public pre-release.

### Added
- Bundled ffmpeg/ffprobe in the installer (NSIS), with third-party notices; system ffmpeg remains the fallback.
- Media processing control: pause, resume and cancel; hardware-accelerated video conversion (auto-detected, tested before use); convert-on-open mode.
- Versioned database migrations (`PRAGMA user_version`) and a file logger with path redaction.
- Paginated, virtualized Chats message pane.
- ffmpeg availability check with a Dashboard warning.
- About panel (version, author, license); MIT license.
- Accessible dialogs (focus trap, roles, Escape) and keyboard-reachable tile actions.

### Changed
- Ingestion runs each phase in one transaction (much faster imports) and is idempotent on re-import.
- Thumbnail generation uses a bounded worker pool (transcodes stay serialized).
- Year filtering is index-backed; the Memories grid no longer re-renders every tile on scroll.
- Inter is bundled locally; a strict Content-Security-Policy and narrower plugin permissions are in place.

### Fixed
- Corrupt videos are marked unplayable once instead of failing on every launch; the playback stats are accurate.
- Memories tiles open the viewer when clicking the bottom strip again; import and thumbnail progress survive switching views.
- A bad overlay image no longer fails the whole thumbnail; failed thumbnails are not retried on every launch.
- Safer timestamp pairing, zip extraction hardening, and guarded reset / storage-move.

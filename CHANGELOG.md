# Changelog

All notable changes to SnapVault. Format: [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
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
- A bad overlay image no longer fails the whole thumbnail; failed thumbnails are not retried on every launch.
- Safer timestamp pairing, zip extraction hardening, and guarded reset / storage-move.

# Backlog

- [ ] Overlay compositing robustness: validate overlay PNG, fall back to base on decode failure (see gotchas.md, 2026-10-21).
- [ ] No automated tests beyond thumbnails.rs; add fixture tests for timestamp repair in `ingestion/memories.rs`.

- Bundle ffmpeg/ffprobe as a Tauri sidecar (currently expected on PATH; Dashboard warns via check_ffmpeg).

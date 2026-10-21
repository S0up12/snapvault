# SnapVault

A self-hosted, offline-first desktop app for browsing your exported Snapchat data: memories, chats and profile. Everything is processed and stored locally on your machine; nothing is uploaded anywhere.

## Features
- Import the Snapchat "Download My Data" `.zip` with streamed progress
- Timestamps, location and tags taken from the export JSON (not EXIF)
- Memories grid with filters, tags, favorites and a year scrubber; overlays composited back onto the original snap
- Chat history and profile views
- Performance and storage settings for low-end hardware

## Requirements
Node.js, Rust (MSVC build tools on Windows) and `ffmpeg`/`ffprobe` on your PATH. On Windows, `scripts/` installs and checks everything: see [scripts/README.md](scripts/README.md).

## Develop
```bash
npm install
npm run tauri dev
```

## Build an installer
```bash
npm run tauri build
```

## Project guidelines
Architecture and contribution rules are in [CLAUDE.md](CLAUDE.md).

## Author
Built by [S0up12](https://github.com/S0up12). Licensed under the [MIT License](LICENSE).

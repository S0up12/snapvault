<div align="center">

<img src="src-tauri/icons/128x128.png" alt="SnapVault" width="96" height="96" />

# SnapVault

**Browse your exported Snapchat data like a real library, offline, on your own machine.**

[![CI](https://github.com/S0up12/snapvault/actions/workflows/ci.yml/badge.svg)](https://github.com/S0up12/snapvault/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Tauri v2](https://img.shields.io/badge/Tauri-v2-24c8db)
![Rust](https://img.shields.io/badge/backend-Rust-b7410e)

</div>

Snapchat's "Download My Data" gives you a pile of zip files, loose media and JSON. SnapVault turns that pile into something you can actually look through: a memories grid, your chats, and your profile.

Everything happens locally. There is no account, no server and no upload. Your export and everything generated from it stay in your own app-data folder.

## Features

- **Import** one or several Snapchat export `.zip` files, with streamed progress for extraction, parsing and media processing.
- **Memories** grid with year scrubber, type filters, favorites and tags, plus a full-screen viewer for photos and video.
- **Original look restored.** Caption and drawing overlays that Snapchat stores as separate images are composited back onto the base photo for thumbnails.
- **Correct dates.** Timestamps, locations and tags come from the export's JSON files, not from media EXIF data, which Snapchat leaves wrong or empty.
- **Chats** with paginated history and linked media, so large conversations stay fast.
- **Profile** view of the account data in your export.
- **Playback-safe video.** HEVC and other formats WebView2 can't play are converted to H.264 in the background.
- **Built for modest hardware.** Conversion speed presets, a CPU-limit switch and a configurable media location.
- **Re-import safe.** Importing the same export again does not duplicate memories or chats.

## Install

Installers will be published on the [Releases](https://github.com/S0up12/snapvault/releases) page once the first version is tagged. Until then, run it from source (below).

**ffmpeg:** SnapVault uses `ffmpeg` and `ffprobe` for thumbnails, overlays and video conversion. The installer bundles them (see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) for licensing). When running from source, they must be on your `PATH`, and the Dashboard shows a warning if they are missing.

SnapVault is developed and tested on Windows 11. The code uses portable paths throughout, but macOS and Linux have not been tested yet.

## Get your Snapchat data

1. Open [accounts.snapchat.com](https://accounts.snapchat.com) and go to **My Data**.
2. Select at least **Memories**, **Chat History** and **Profile** data, and make sure the **JSON** files are included. SnapVault needs `memories_history.json` and the other JSON files for correct dates and metadata.
3. Submit the request. Snapchat emails you a link when the export is ready. Large exports are split into several zip files, so download all of them.
4. In SnapVault, choose **Select Snapchat export (.zip)** on the Dashboard and select every part at once.

## Run from source

Prerequisites: [Node.js](https://nodejs.org) 20.19+ (CI uses 22), [Rust](https://rustup.rs) (on Windows with the MSVC build tools) and `ffmpeg` on your `PATH`.

```bash
git clone https://github.com/S0up12/snapvault.git
cd snapvault
npm install
npm run tauri dev
```

On Windows, the scripts in [`scripts/`](scripts/README.md) install the toolchain, check your environment and start the app for you.

Build an installer:

```bash
npm run tauri build
```

Checks used by CI:

```bash
npx tsc --noEmit
cargo test --manifest-path src-tauri/Cargo.toml
```

## How it works

| Layer | Technology |
|---|---|
| App shell | Tauri v2 |
| Frontend | React, TypeScript, Tailwind CSS, Vite |
| Backend | Rust (all heavy work on background threads) |
| Storage | SQLite via `rusqlite`, with versioned migrations |
| Media | `ffmpeg` / `ffprobe` |

- The frontend only talks to the backend through Tauri commands, and the UI thread never does heavy work.
- Zip extraction streams entries to disk with size and path-safety limits, so large archives don't exhaust memory.
- Import phases run in single database transactions, and media is processed by a small worker pool. Video conversion runs one file at a time.
- Media paths are stored relative to the media root, so the library can be moved.
- A strict Content-Security-Policy and narrow plugin permissions limit what the webview can do.

```
src/            React app (views, components, hooks, lib)
src-tauri/src/  Rust backend (ingestion, thumbnails, db, library, chats, settings)
scripts/        Windows setup and dev helper scripts
```

## Privacy

SnapVault makes no network requests of its own. Fonts are bundled, there is no analytics or telemetry, and imported data never leaves your device. Logs are written to a local file in the app-data folder with file paths redacted, and never contain message text.

## Known limitations

- Stories are not imported yet. The navigation entry is hidden until they are.
- Overlays are composited for thumbnails but not for full-size viewing or for chat media.
- Import and thumbnail runs cannot be cancelled once started.
- If a day's JSON entries and files don't match in number, those memories get the day's date but no exact time.

The full list lives in [`.claude/memory/backlog.md`](.claude/memory/backlog.md).

## Contributing

Issues and pull requests are welcome. Architecture notes and project rules are in [CLAUDE.md](CLAUDE.md). The repo also ships a small multi-agent setup for Claude Code in [`.claude/`](.claude) (specialist agents, skills and shared memory) that you can ignore if you don't use it.

## Disclaimer

SnapVault is an independent project and is not affiliated with, endorsed by or sponsored by Snap Inc. Snapchat is a trademark of Snap Inc.

## License

Released under the [MIT License](LICENSE). Built by [S0up12](https://github.com/S0up12).

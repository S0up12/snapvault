# Gotchas

- Commands compile but are unreachable unless added to `generate_handler!` in `lib.rs`. **Apply:** every new command.
- Tauri maps Rust `snake_case` args to JS `camelCase`. **Apply:** invoke bindings.
- On Windows, spawn ffmpeg/ffprobe only via `no_window_command` (CREATE_NO_WINDOW) or a console window flashes per spawn.
- ffmpeg picks the output muxer from the extension: temp files must end `.mp4`/`.webp`, not `.x.tmp`.
- Only h264 + aac/mp3 in .mp4/.m4v is treated as playback-safe in WebView2; everything else (HEVC, .mov) is transcoded.
- Observed 2026-10-21: during a dev run, overlay compositing logged `Decode error rate 1 exceeds maximum` / `Cannot determine format of input` for a PNG overlay and the webp output was not written. Likely a corrupt/zero-byte or non-PNG overlay file. **Apply:** thumbnails.rs overlay step should validate the overlay and fall back to the base image instead of failing. Unconfirmed root cause.
- `.claude/` is partly committed (agents, skills, memory); `settings.local.json` stays ignored.

---
name: sv-verify
description: Commands to verify SnapVault changes compile and type-check. Use after any Rust or TypeScript edit, before reporting done.
---
# Verify

Run only what the change touches. Run from the repo root.

| Changed | Command | Notes |
|---|---|---|
| Rust | `cargo check --manifest-path src-tauri/Cargo.toml` | fast; first run is slow |
| Rust logic | `cargo test --manifest-path src-tauri/Cargo.toml <filter>` | thumbnail tests need `ffmpeg` on PATH |
| Rust lint (optional) | `cargo clippy --manifest-path src-tauri/Cargo.toml` | |
| Frontend | `npx tsc --noEmit` | |
| Frontend build | `npm run build` | only before release |

Do NOT run `npm run tauri dev` to verify: it is long-running and opens a window. Ask the user to run it, or use the `run` skill when a live check is truly required.
Report failures with the first error only; don't paste full logs.

---
name: sv-add-command
description: Checklist for adding or changing a Tauri IPC command end to end in SnapVault (Rust fn, registration, TS hook).
---
# Add a Tauri command

1. **Rust:** in the owning module in `src-tauri/src/` add `#[tauri::command] pub fn name(state: tauri::State<DbState>, ...) -> Result<T, String>`. Map errors with `.map_err(|e| e.to_string())`. Return types derive `Serialize` (`#[serde(rename_all = "camelCase")]` if the TS side expects camelCase - check a sibling struct). Use `async fn` + a background thread for anything slow.
2. **Register:** add to `tauri::generate_handler![...]` in `src-tauri/src/lib.rs`. Forgetting this is the #1 failure ("command not found" at runtime, compiles fine).
3. **Schema change?** append a fn to `MIGRATIONS` in `db/mod.rs` (runs once in a transaction, bumps `PRAGMA user_version`). Never edit `schema.rs` (frozen v1 baseline) or shipped migrations; add a test in `db/mod.rs`.
4. **Frontend:** add a hook in `src/hooks/` calling `invoke<T>("name", { camelCaseArgs })`. Tauri converts Rust `snake_case` args to JS `camelCase`. Mirror the Rust struct as a TS type in the same hook file.
5. **Events:** progress streams use `app.emit("x://progress", payload)` and `listen` in the hook; clean up the listener on unmount.
6. Update `.claude/memory/architecture.md` command list. Verify with `sv-verify`.

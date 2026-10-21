# Decisions and measurements

- Agent roster is deliberately small (6) and model-tiered for Claude Team plan limits: haiku for lookup, sonnet for work, opus only on explicit escalation. Sequential by default.
- Legacy Python repo is a read-only behavior reference, not a dependency.
- Ingestion perf (temp-file WAL db, debug build, 5k memories + 20k msgs, test `bench_parse_insert_phase`): autocommit baseline memories 24.5s / chats 41.4s -> one `unchecked_transaction` per phase (memories.rs, chats.rs; rollback on `?`) 4.1s / 1.1s. `synchronous=NORMAL` gave no measurable extra gain (4.1/1.16 vs 3.8-4.1/1.10) so NOT adopted; WAL was already on (db::PRAGMAS). Remaining memories time is file I/O (timestamp repair), not SQL. profile.rs is a single upsert, left alone.
- Memories grid render perf (analytic, no test runner in repo; not measured in-app): before, every visibleYear change re-rendered Memories, creating a new onToggleFavorite arrow, so all visible memo(TimelineTile) re-rendered per year change and per favorite toggle (visible tiles N -> N). After: Memories uses a useCallback handleToggleFavorite (toggleFavorite is already stable; setSelectedIndex/setTagEditorAsset are stable), so a year change re-renders 0 tiles (Memories re-render only) and a favorite toggle re-renders 1 tile (only the asset whose object changed). VirtualTimelineGrid scroll listener now attaches once per scrollElement, latest state read through a ref, rAF-throttled, and onVisibleYearChange fires only when the year changes. Risk: check() runs one frame after scroll; fetch threshold is 1200px so negligible.

## Perf: year filter sargable (library.rs), 50k memory assets, temp-file DB, per-call count+page / list_years
- Was `substr(taken_at,1,4)=:year` (full scan of source_type partition). Now `taken_at >= :year AND taken_at < CAST(:year+1)` (ISO text prefix compare), same ids verified.
- all/2016: 37.3ms -> 2.4ms; photo/2020: 18.5 -> 1.9ms; list_years(year): 13.5 -> 1.5ms.
- favorite+year regressed 44 -> 64ms until adding index ix_assets_source_type_favorite_taken_at (source_type,is_favorite,taken_at) -> 1.9ms. Added via CREATE INDEX IF NOT EXISTS in SCHEMA_SQL.
- list_memory_tags already skips tags='[]'; not changed (not measured as a bottleneck). count/page left separate.
- Bench: `cargo test bench_year -- --nocapture`.

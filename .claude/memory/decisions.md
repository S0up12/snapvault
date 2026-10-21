# Decisions and measurements

- Agent roster is deliberately small (6) and model-tiered for Claude Team plan limits: haiku for lookup, sonnet for work, opus only on explicit escalation. Sequential by default.
- Legacy Python repo is a read-only behavior reference, not a dependency.
- Ingestion perf (temp-file WAL db, debug build, 5k memories + 20k msgs, test `bench_parse_insert_phase`): autocommit baseline memories 24.5s / chats 41.4s -> one `unchecked_transaction` per phase (memories.rs, chats.rs; rollback on `?`) 4.1s / 1.1s. `synchronous=NORMAL` gave no measurable extra gain (4.1/1.16 vs 3.8-4.1/1.10) so NOT adopted; WAL was already on (db::PRAGMAS). Remaining memories time is file I/O (timestamp repair), not SQL. profile.rs is a single upsert, left alone.

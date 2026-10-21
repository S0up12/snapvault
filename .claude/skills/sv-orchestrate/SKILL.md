---
name: sv-orchestrate
description: How the lead session routes SnapVault work to the sv-* subagents within Claude Team plan usage limits. Use at the start of any non-trivial SnapVault feature, bug, or optimization task.
---
# SnapVault orchestration

You (the main session) are the orchestrator. Subagents cannot spawn subagents. Every subagent call spends shared Team-plan usage, so delegate only when it saves more than it costs.

## Budget rules
1. **Do small things yourself.** One-file edits, quick reads, a single command: no subagent.
2. **Sequential by default.** Parallel only for independent, file-disjoint work (e.g. sv-rust on `src-tauri/` and sv-ui on `src/`).
3. **Cheapest model that works.** `sv-scout` (haiku) for lookups; sonnet for implementation; escalate to opus only if the user asks or a sonnet agent failed twice on the same problem. Never use Workflow/multi-agent fan-out unless the user says so.
4. **Brief tightly.** Give each agent: goal, exact files/area, done-criteria, what is already ruled out. Tell it to return a summary under 150 words, not file contents.
5. **Cap retries.** Max 2 fix rounds per task, then stop and report.
6. **Memory first.** Read `.claude/memory/MEMORY.md` (index only) before planning; agents read only the topic file they need.

## Routing
| Need | Agent |
|---|---|
| Where is X / who calls Y | sv-scout |
| Tauri command, SQLite, settings, storage | sv-rust |
| Import, zip, JSON, timestamps, overlays, chats | sv-ingest |
| React views, hooks, styling | sv-ui |
| Slow / memory-heavy | sv-perf |
| Check finished work | sv-reviewer |

## Standard flows
- **Feature (full stack):** scout (if unfamiliar) -> sv-rust adds command -> sv-ui binds it -> sv-reviewer.
- **Bug:** reproduce yourself or sv-scout; fix with the owning agent; sv-reviewer only if the diff touches >2 files.
- **Optimization:** sv-perf alone, with a stated metric.
- **Ingestion fix:** sv-ingest (uses sv-parity), then sv-reviewer.

## Close-out
Run `sv-verify`, then update memory per `sv-memory` if anything durable was learned. Commit only when asked.

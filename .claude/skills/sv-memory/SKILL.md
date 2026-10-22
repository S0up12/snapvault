---
name: sv-memory
description: How Snapvault agents read and write the shared project memory in .claude/memory. Use at task start (read index) and task end (record durable learnings).
---
# Shared memory

Location: `.claude/memory/` (committed to git, so it travels with the repo).

- **Read:** open `MEMORY.md` (the index, ~20 lines). Open only the topic file relevant to the task. Never load all of them.
- **Write** only durable, non-obvious facts: gotchas, decisions + why, Snapchat-export quirks, measured perf numbers, backlog items. Not task diaries, not things visible in code or `git log`.
- **Format:** one bullet per fact, `- fact. **Why:** reason. **Apply:** when.` Keep each file under ~80 lines; merge duplicates and delete stale facts instead of appending.
- **Files:** `architecture.md` (map), `gotchas.md` (traps), `decisions.md` (choices + perf results), `backlog.md` (open work).
- If you add a file, add its one-line pointer to `MEMORY.md`.
- Never record user data, paths to real archives, or secrets.

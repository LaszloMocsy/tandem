---
id: u89nzjq
title: Demo kit for the hero recording
status: done
priority: medium
parent: ybd67we
labels: [area:website]
depends_on: [itnff5i]
created: 2026-10-22
reviewed_at: 2026-10-22
---

## Description
Everything needed to record the same hero clip every take, in `site/demo/`.

- `seed.ts`: builds a throwaway repo at `/tmp/tandem-demo` for a small CLI called `pocket-weather`, with real git history and about 9 tasks: Backlog 2 (one agent-proposed), Todo 2 including **Add a `--units` flag** (high, 3 criteria), In progress 1 held by a `codex-…` agent, Review 1, Done 3. Seeding must go through Tandem (MCP or the app), not by writing task files by hand.
- `drive.ts`: calls the real MCP tools on a fixed timeline, reporting its client name as `claude-code`: `start_task` at 2s, two `log_progress` calls between 4s and 8s, `finish_task` with all criteria met at about 8s. Takes the project's MCP URL as an argument.
- `RECORDING.md`: the shot list for the 18s hero loop (board at rest → In progress with live badge → progress → Review → user opens card → presses R → Done → fade to the first frame), optional clips A (task file diff), B (Setup panel copy), C (agent proposes a child task, user approves with A), and the checklist: a release build (`cargo tauri build`, so no DEV strip), window 1440×900, light take then dark take, clean desktop, slow cursor, 60fps, crop to the window. Include `ffmpeg` commands for AV1 WebM and H.264 MP4 under about 1.5 MB each, plus a poster frame at 3s.

## Acceptance criteria
- [x] `seed.ts` builds the demo repo from scratch and can be rerun to reset it
- [x] `drive.ts` runs the hero timeline against a running app, and the board moves as scripted
- [x] `RECORDING.md` has the shot list, the optional clips, the checklist and the encode commands
- [x] The kit was run end to end once, and any timing changes are written into `RECORDING.md`

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-7856 · started

### 2026-10-22 · claude-code-7856 · finished
Added the demo kit in site/demo/ plus a seeder example in core, per the user's choice of a Rust seeder (approve and mark_reviewed aren't MCP tools). seed.ts refuses to delete a folder it didn't make (marker inside .git), scaffolds pocket-weather with an initial commit, and runs `cargo run -p tandem-board-core --example demo_seed`, which calls the core Service in a throwaway index: commits .tandem/, then 3 Done tasks (create, start, code commit with the in_progress file, finish, finished commit, mark_reviewed, commit_review), 1 in Review, 3 in Todo (incl. "Add a --units flag", high, 3 criteria), 2 agent-proposed in Backlog, and a final plan commit, leaving a clean tree. drive.ts is a dependency-free Streamable HTTP MCP client (JSON or SSE replies): it claims "Retry failed requests with backoff" as codex and heartbeats it every 60s, waits for Enter (or --now), then as claude-code runs start_task at 2s, log_progress at 4.5s and 6.5s, finish_task with met 1-3 at 8s. RECORDING.md has the board table, setup, per-take steps, the 18s shot list, optional clips A-C, the checklist (release build, 1440x900 via osascript, light then dark, DND, 60fps) and ffmpeg commands (lossless master with a 1s crossfade to the first frame, AV1 WebM, H.264 MP4, poster at 3s). Ran end to end against the user's app with /tmp/tandem-demo open: the timeline hit 2.0/4.5/6.5/8.0s, MCP list_tasks showed --units in Review and Codex's live lease; reseeding with the project open and driving again also worked. cargo fmt, clippy -D warnings and the site check/test/build pass.
Files: core/examples/demo_seed.rs, site/demo/seed.ts, site/demo/drive.ts, site/demo/RECORDING.md
Follow-ups: The task asked for Backlog with "one agent-proposed"; both backlog tasks are agent-created, since a task you create goes straight to Todo. The ffmpeg commands weren't run here (no ffmpeg installed), so ty8sxn6 should check crop, sizes and the crossfade on the first real take. I verified the board state over MCP but didn't watch the app window, and the human beats (~9.5s open, ~12s R) are estimates for the person recording. demo_seed is an example, not library code, so it has no unit tests of its own; running seed.ts exercises it.

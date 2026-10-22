---
id: u89nzjq
title: Demo kit for the hero recording
status: todo
priority: medium
parent: ybd67we
labels: [area:website]
depends_on: [itnff5i]
created: 2026-10-22
---

## Description
Everything needed to record the same hero clip every take, in `site/demo/`.

- `seed.ts`: builds a throwaway repo at `/tmp/tandem-demo` for a small CLI called `pocket-weather`, with real git history and about 9 tasks: Backlog 2 (one agent-proposed), Todo 2 including **Add a `--units` flag** (high, 3 criteria), In progress 1 held by a `codex-…` agent, Review 1, Done 3. Seeding must go through Tandem (MCP or the app), not by writing task files by hand.
- `drive.ts`: calls the real MCP tools on a fixed timeline, reporting its client name as `claude-code`: `start_task` at 2s, two `log_progress` calls between 4s and 8s, `finish_task` with all criteria met at about 8s. Takes the project's MCP URL as an argument.
- `RECORDING.md`: the shot list for the 18s hero loop (board at rest → In progress with live badge → progress → Review → user opens card → presses R → Done → fade to the first frame), optional clips A (task file diff), B (Setup panel copy), C (agent proposes a child task, user approves with A), and the checklist: a release build (`cargo tauri build`, so no DEV strip), window 1440×900, light take then dark take, clean desktop, slow cursor, 60fps, crop to the window. Include `ffmpeg` commands for AV1 WebM and H.264 MP4 under about 1.5 MB each, plus a poster frame at 3s.

## Acceptance criteria
- [ ] `seed.ts` builds the demo repo from scratch and can be rerun to reset it
- [ ] `drive.ts` runs the hero timeline against a running app, and the board moves as scripted
- [ ] `RECORDING.md` has the shot list, the optional clips, the checklist and the encode commands
- [ ] The kit was run end to end once, and any timing changes are written into `RECORDING.md`

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

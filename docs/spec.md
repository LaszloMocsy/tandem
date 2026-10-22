# Agent Task Board — v1 Spec

Oct 13, 2026 · @László Mócsy

## Overview

Tandem (working name) is a local Tauri desktop app with a kanban board for tasks that you and AI coding agents share. Tasks are Markdown files in the repo, agents work on them through an MCP server built into the app, and you review and tick off what the agents finish.

Goals for v1:

- One task = one Markdown file in `.tandem/`, committed together with the code that works on it
- Agents claim, work on and complete tasks through MCP tools, safely in parallel
- A board with Backlog, Todo, In progress, Review and Done columns, showing live agent activity
- A review tick that only you can set after an agent marks a task finished
- Open source on your personal GitHub, buildable in about two days

What v1 leaves out is listed under Scope and later versions.

## Core principles

Git records outcomes, SQLite holds activity. Every other rule follows from that.

1. **A task file changes only when something worth reading later happens:** the task is created, its spec is edited, it is approved, started, finished, reviewed, reopened, released or deleted. Claims, heartbeats, blocked flags and the working log never touch files.
2. **The app commits only when you press Commit review.** Otherwise it only writes files. The task file rides along in the commit with the code: the first commit of a task carries `status: in_progress`, the last one carries `status: finished`, and your review then sets `done`.
3. **Files are the source of truth.** The SQLite database can be deleted at any time and is rebuilt from `.tandem/`. Only in-flight state (leases, working logs) is lost.
4. **Nothing in the repo except `.tandem/`.** SQLite lives in the app's data directory, so the repo needs no `.gitignore` entries.
5. **Agents change tasks only through MCP tools**, never by editing frontmatter or the changelog by hand. You can edit task files in any editor; the app picks up the change.
6. **Small, stable diffs.** The app writes frontmatter in a fixed key order and touches only the lines it owns.

## Task file format

Each task is one Markdown file with YAML frontmatter, stored flat in `.tandem/` at the repo root.

```txt
.tandem/
  config.toml    # format_version and the board's name, created on init
  README.md      # format reference for humans and agents, created on init
  a7rq4ne.md     # a parent task
  k3f9x2m.md
  p2xmq7d.md
```

**IDs** are 7 random characters from a 32-character alphabet (lowercase letters and digits without `0 o 1 l`), such as `k3f9x2m`. That gives about 34 billion possible ids, so tasks created on different branches practically never clash; the app still checks before creating one. The filename is exactly `<id>.md`. The app reads only files whose name matches that pattern, so `README.md` and anything else in the folder is ignored. People and agents find tasks through the app or the MCP tools, by id, title, label or text, never by filename.

**Project settings.** `.tandem/config.toml` holds the settings the whole team shares. The app creates it on init, and it is committed like the tasks.

```toml
format_version = 1
name = "Tandem"
```

`format_version` records which version of this file format the folder uses. On open, the app compares it with the versions it supports. An older version gets a one-time migration that rewrites the files, which you then commit. A newer version opens the project read-only and asks you to update the app. A missing file is treated as version 1 and written.

`name` is the board's display name, set in Settings. It is optional: without it, the app uses the repo folder's name. Writes to `config.toml` follow the task-file write rules: they keep the other keys, comments and order, change only the `name` line, and write atomically. An invalid `config.toml` opens the project read-only and is never overwritten.

**Example file:**

```markdown
---
id: k3f9x2m
title: Fix token refresh race
status: finished
priority: high
parent: a7rq4ne
labels: [auth, backend]
depends_on: [p2xmq7d]
created: 2026-10-13
---

## Description
Parallel requests that hit a 401 each trigger their own token refresh.

## Acceptance criteria
- [x] Concurrent requests during a refresh share one refresh call
- [x] A test covers two parallel 401 responses

## Relevant files
- src/auth/session.ts

## Changelog

### 2026-10-13 · claude-code · started
Picked up from todo.

### 2026-10-14 · claude-code · finished
Serialized refresh behind a shared promise and added a parallel-401 test.
Files: src/auth/session.ts, src/auth/session.test.ts
Follow-ups: none
```

**Frontmatter fields,** written in this order:

| Field | Type | Default | Written by | Notes |
| --- | --- | --- | --- | --- |
| `id` | 7-char string | generated | app on create | Never changes; also the filename |
| `title` | string | required | you or agent | |
| `status` | `backlog` · `todo` · `in_progress` · `finished` · `done` | `backlog` | app, through tools or the board | The only committed statuses. Agent-created tasks start in `backlog`, yours in `todo` |
| `priority` | `high` · `medium` · `low` | none | you or agent | Optional |
| `parent` | task id | none | you or agent | Groups this task under another. Must exist and must not form a loop. Omitted when empty |
| `labels` | list of strings | none `[``]` | you or agent | Omitted when empty |
| `depends_on` | list of task ids | none `[]` | you or agent | `start_task` refuses until all are `done`. Must not form a loop. Omitted when empty |
| `created` | date | today | app on create | |
| `reviewed_at` | date | none | app, when you tick | Set when you review the task and mark it done. Removed whenever status leaves `done` |

**Body sections:** `Description`, `Acceptance criteria` (checkboxes, ticked through finish\_task), `Relevant files` and `Changelog`. Unknown sections are kept untouched. The changelog is owned by the app and append-only, oldest entry first, so diffs only ever add lines at the end. Each entry is `### <date> · <who> · <event>`, then a summary, then optional `Files:`, `Follow-ups:` and `Unmet:` lines.

**Parents and loops.** `parent` builds a tree of any depth, and `depends_on` builds a graph. Neither may contain a loop: a task that is its own ancestor, or a dependency chain that leads back to its start. The app checks on every write from the board or MCP and refuses with `cycle_detected`. It checks again after every rescan, because hand edits and merges can bring a loop in. Tasks in a loop, or whose parent is missing, get an invalid badge and show at the top level until fixed. A parent's status is not derived from its children; its card shows their progress, such as 3/5 done.

**Write rules:**

- Write atomically: a temp file in `.tandem/`, then rename
- LF line endings and one trailing newline; keep the body byte for byte except the changelog append
- If a file fails to parse, mark it invalid on the board and never overwrite it
- Never follow symlinks: a symlinked `.tandem` opens the project read-only, and a symlinked task file is invalid. Changelog summaries can't contain headings, unclosed code fences, or lines starting with `Files:`, `Follow-ups:` or `Unmet:`

## State model

Five statuses are committed, and reviewed\_at records when you approved a task; everything else is a local overlay in SQLite.

&#91;embedded content: task states · 5 columns, 6 transitions\]

You approve tasks into Todo and review them into Done; agents only work on what sits in between.

**Board columns** map one to one onto statuses: Backlog = `backlog`, Todo = `todo`, In progress = `in_progress`, Review = `finished`, Done = `done` (always with `reviewed_at`). Agents can only start `todo` tasks, so nothing an agent proposes gets worked on before you approve it.

**Local overlays** (never committed) show on In progress cards:

- **Lease:** which agent holds the task and until when. Live, stale (expired), or none (started by you or after a rebuild).
- **Blocked:** a reason set by the agent, shown as a badge until cleared.
- **Working log:** the agent's step-by-step notes, plus your reopen feedback.

**Transitions:**

| Transition | Trigger | Who | Task file change | SQLite change |
| --- | --- | --- | --- | --- |
| New task | `create_task`, or the New task dialog | agent or you | New file in `backlog` (agent) or `todo` (you), changelog `created` | none |
| Backlog → Todo | Approve, or drag | you only | `status: todo`, changelog `approved` | none |
| Todo → In progress | `start_task`, or drag | agent or you | `status: in_progress`, changelog `started` | Lease created (agent only) |
| In progress → In progress | any tool call by the lease holder | agent | none | Lease extended |
| Blocked on / off | `set_blocked` | agent | none | Flag set or cleared, log entry |
| In progress → Review | `finish_task`, or drag | agent or you | `status: finished`, changelog `finished` with summary | Lease released |
| Review → Done | Mark reviewed | you only | `status: done`, `reviewed_at: <today>` | none |
| Review → In progress | Reopen with feedback | you only | `status: in_progress`, changelog `reopened` with feedback | Feedback stored for the next agent |
| In progress → Todo | `release_task`, or drag | agent or you | `status: todo`, changelog `released` | Lease removed |
| Delete | Delete task, from any status | you only | File deleted | Index row and any stale lease removed |

Any move out of `done` also removes `reviewed_at`.

No MCP tool can approve a task into `todo` or set `reviewed_at`, so agents cannot approve their own work. No MCP tool deletes a task either.

Deleting is refused while an agent holds a live lease on the task, and while another task names it as its `parent` or lists it in `depends_on`, since that task would be left invalid or locked for good; you unlink those first. An invalid file can be deleted. Like any other write, the deletion is not committed: git history keeps the last committed version, and the confirmation warns when the file was never committed. Marking reviewed writes the file without committing it: it lands in your next commit, you can amend it into the latest commit, or you can press **Commit review** to commit only that file.

## SQLite schema

SQLite lives in the app's data directory, never in the repo: one global `app.db` for the project list and the app settings, plus one database per project at `projects/<project-id>.db`. Both use WAL mode. Every row in a project database is either rebuilt from files or deliberately short-lived.

```sql
-- app.db
CREATE TABLE projects (
  id             TEXT PRIMARY KEY,   -- short slug, used in the MCP URL; unique on this machine
  name           TEXT NOT NULL,      -- copy of config.toml's name, or the folder name
  repo_path      TEXT NOT NULL UNIQUE,
  last_opened_at TEXT
);

CREATE TABLE settings (              -- app settings, such as mcp_port and theme
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- projects/<project-id>.db
CREATE TABLE tasks (                 -- index of .tandem/*.md, rebuilt from files
  id          TEXT PRIMARY KEY,
  path        TEXT NOT NULL,
  title       TEXT NOT NULL,
  status      TEXT NOT NULL CHECK (status IN ('backlog','todo','in_progress','finished','done')),
  priority    TEXT NOT NULL DEFAULT 'medium',
  parent      TEXT,                         -- task id, NULL at the top level
  labels      TEXT NOT NULL DEFAULT '[]',   -- JSON array
  depends_on  TEXT NOT NULL DEFAULT '[]',   -- JSON array of task ids
  created     TEXT NOT NULL,
  reviewed_at TEXT,                         -- NULL means not reviewed
  body        TEXT NOT NULL,
  file_mtime  INTEGER NOT NULL,             -- ms since epoch, for the stat check
  file_size   INTEGER NOT NULL,             -- bytes, for the stat check
  file_hash   TEXT NOT NULL,                -- sha256 of the file as last read or written
  parse_error TEXT                          -- invalid file, loop, or missing parent
);
CREATE INDEX tasks_by_parent ON tasks (parent);

CREATE TABLE leases (
  task_id    TEXT PRIMARY KEY,
  agent_id   TEXT NOT NULL,
  claimed_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);

CREATE TABLE task_flags (
  task_id         TEXT PRIMARY KEY,
  blocked_reason  TEXT,
  review_feedback TEXT                      -- latest reopen feedback for the next agent
);

CREATE TABLE work_log (
  id         INTEGER PRIMARY KEY,
  task_id    TEXT NOT NULL,
  agent_id   TEXT,                          -- NULL means you
  kind       TEXT NOT NULL CHECK (kind IN ('note','blocked','unblocked','feedback','event')),
  message    TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX work_log_by_task ON work_log (task_id, created_at);

CREATE TABLE agents (
  agent_id     TEXT PRIMARY KEY,
  client_name  TEXT,                        -- from MCP clientInfo, such as claude-code
  last_seen_at TEXT NOT NULL
);
```

`leases`, `task_flags` and `work_log` deliberately have no foreign key to `tasks`, so they survive a reindex.

**Rebuild rules:**

- **No hash inside task files.** Checking one would still mean reading the whole file, and every edit would rewrite that line and cause merge conflicts. Change detection uses `file_mtime`, `file_size` and `file_hash` in SQLite instead.
- **Stat check:** every scan compares each file's mtime and size with the index, and re-reads and re-hashes only files where either changed. If the hash is unchanged, only the stored mtime is updated.
- **On project open:** a full scan of `.tandem/`, then index rows whose file is gone are deleted. This catches everything that changed while the app was closed.
- **On a watcher event:** re-parse that file, debounced by about 200 ms. A burst of events (branch switch, `git pull`) triggers a full scan.
- **Every 60 seconds:** a stat-only sweep, for events the watcher missed (sleep, network drives).
- **After any index change:** re-check the whole graph for parent loops, dependency loops and missing parents. It is a linear pass, so it needs no separate trigger.
- Drop any lease whose task is no longer `in_progress` in its file, for example after switching branches.
- Delete working-log rows for missing tasks after 30 days.
- A Reset local state button in Settings deletes the project database and rescans. An agent working on a task carries on with its id, as after any rebuild.

## MCP server

The app serves MCP over Streamable HTTP on `127.0.0.1:4317` (the port is set in Settings), with one endpoint per project: `http://127.0.0.1:4317/p/<project-id>/mcp`. It binds to localhost only and needs the app to be running. If the call fails to connect, the agent tells you to open the app.

**Agent identity:** issued by the server. `start_task` creates a new agent id from the MCP client name and a random suffix (such as `claude-code-7f2a`) and returns it; agents cannot choose their own. The tools only the lease holder may call take it back as a required `agent` argument, and the other tools accept it optionally. A call without it is attributed to the client name from `clientInfo`, such as `claude-code`. Identity doesn't depend on MCP sessions, so clients on protocol 2026-07-28, which has none, and older clients with sessions work the same way. **Leases** last 15 minutes and are extended by every tool call that passes the holder's id, so agents never need to send a separate heartbeat.

**Tools:**

| Tool | Arguments | What it does | Writes file |
| --- | --- | --- | --- |
| `list_tasks` | `status?`, `label?`, `parent?`, `query?`, `ready_only?`, `agent?` | Summaries: id, title, status, priority, parent, labels, lease holder, blocked | no |
| `get_task` | `id`, `agent?` | Full Markdown, parent and children, working log, latest review feedback | no |
| `next_task` | `label?`, `parent?`, `agent?` | Highest-priority todo task with all dependencies done and no live lease | no |
| `create_task` | `title`, `description`, `acceptance_criteria[]`, `priority?`, `parent?`, `labels?`, `depends_on?`, `agent?` | New file in backlog, changelog `created` | yes |
| `update_task` | `id` plus any of `title`, `description`, `acceptance_criteria`, `relevant_files`, `priority`, `parent`, `labels`, `depends_on`; `agent?` | Edits the spec; cannot change status or review | yes |
| `start_task` | `id` | Atomic claim; sets `in_progress`, changelog `started`; returns the new agent id | yes |
| `log_progress` | `id`, `message`, `agent` | Adds a working-log note | no |
| `set_blocked` | `id`, `reason` (empty clears it), `agent` | Sets or clears the blocked flag | no |
| `add_changelog_entry` | `id`, `summary`, `files?`, `agent` | Optional milestone entry before an intermediate commit on a long task | yes |
| `finish_task` | `id`, `summary`, `files_changed[]`, `follow_ups?`, `met?`, `unmet?`, `agent` | Ticks the `met` criteria, sets `finished`, changelog `finished`, releases the lease. Refuses with `criteria_unmet` while an unticked criterion is missing from `unmet` | yes |
| `release_task` | `id`, `note`, `agent` | Sets `todo`, changelog `released`, removes the lease | yes |

`create_task` and `update_task` refuse a `parent` or `depends_on` that would close a loop.

`update_task` from an agent is limited so that nothing an agent changes gets worked on before you approve it. In backlog, agents can edit anything. After approval, `relevant_files`, `labels` and `priority` stay open; any other change to a todo task sends it back to backlog with an `edited` changelog entry until you approve it again, and is refused on a task in progress, which only the lease holder can edit at all. Finished and done tasks are closed to agents. You can edit any task from the board.

No tool approves a task into `todo` or sets `reviewed_at`; both are yours, on the board.

**Tick or explain.** In `finish_task`, `met` lists the acceptance criteria (by number) the agent met, and the app ticks them. Every criterion still unticked must appear in `unmet` as `{criterion, reason}`, or the call fails with `criteria_unmet`. The reasons go into the changelog entry as an `Unmet:` line, and the Review card shows a badge such as "2 criteria unmet". You then accept the task or reopen it. A tick is still the agent's own claim; your review and the tests are the real check. Tasks without criteria always pass.

**Claim rule:** `start_task` runs in one `BEGIN IMMEDIATE` transaction. It fails if the task is not in todo, if another agent holds a live lease, or if any dependency is not `done`. A stale lease can be taken over, and the changelog notes the takeover. An in-progress task with no lease can be started by an agent only if you reopened it; a task you started from the board stays yours. If a rebuild lost a lease, the agent that started the task carries on with its id, and its next holder call restores the lease. Calling `start_task` again on a held task is refused with `claimed_by_other`, since every call issues a new id. Only the lease holder (or you, from the board) may call `log_progress`, `set_blocked`, `add_changelog_entry`, `finish_task` or `release_task`. Agent ids are 1 to 64 ASCII letters, digits, `-`, `_` or `.`, and must not match your name in any case, so agent entries can't pass for yours.

**Errors** are structured with a code the agent can act on: `not_found`, `claimed_by_other`, `deps_not_done`, `not_holder`, `invalid_file`, `invalid_transition`, `cycle_detected`, `parent_not_found`, `criteria_unmet`. Bad arguments return `invalid_argument`, writes to a read-only project return `read_only`, and anything else is `internal`.

**Every file-writing tool returns the changed path** and a reminder to include it in the next commit. Tool descriptions repeat the commit rules, so agents follow them even without an `AGENTS.md`.

## Agent workflow

An agent's task always runs in this order, and its first and last commits carry the task file.

1. Pick a task: the id you were given, or `next_task`. Read it with `get_task`.
2. `start_task`. The first commit for this work includes the task file with `status: in_progress`.
3. Work. Use `log_progress` for notes, and `set_blocked` with a question for you when stuck.
4. On a long task, call `add_changelog_entry` before a meaningful intermediate commit (optional).
5. `finish_task` with a summary and every acceptance criterion ticked or explained, then make the final commit including the task file with `status: finished`.
6. If you reopen it, `get_task` shows your feedback; the agent calls `start_task` again.

**Connecting an agent.** The app's setup panel generates the exact config per project. For Claude Code:

```txt
claude mcp add --transport http tandem http://127.0.0.1:4317/p/<project-id>/mcp
```

Or commit a project-level `.mcp.json`:

```json
{
  "mcpServers": {
    "tandem": { "type": "http", "url": "http://127.0.0.1:4317/p/<project-id>/mcp" }
  }
}
```

Other clients (Codex, Cursor) use their own config formats; check each client's current MCP docs when building the setup panel.

**Snippet for `AGENTS.md` or `CLAUDE.md`:**

```markdown
## Tasks

This repo tracks work in `.tandem/` through the `tandem` MCP server.

- Before starting work, call `start_task` on the task (use `next_task` if none was given), and pass the agent id it returns as `agent` to every later tandem call for that task.
- Never edit task frontmatter or the Changelog section by hand. Use the tandem tools.
- Include the task file in your first commit (status in_progress) and your final commit (status finished).
- Add a `Task: <id>` trailer to every commit for that task.
- Use `log_progress` for notes and `set_blocked` when you need a decision from me.
- Split large work into child tasks with `create_task` and `parent`. New tasks wait in backlog until I approve them; only start tasks in todo.
- Call `finish_task` with a short summary before the final commit. Tick each acceptance criterion you met and explain each one you didn't. Never mark tasks reviewed.
- If the tandem server is unreachable, ask me to open the app instead of editing files.
```

## App architecture

One Tauri 2 app: a Rust core owns every file write and database change, and both the UI and the MCP server go through it.

&#91;embedded content: app architecture · UI and MCP server over one task service\]

The board talks to the task service through Tauri commands and events; agents reach it over local HTTP; the watcher feeds outside edits back in.

**Stack:**

| Layer | Choice | Why |
| --- | --- | --- |
| Shell | Tauri 2 | Small native binary; Rust core and web UI in one app |
| Frontend | React + TypeScript + Vite, dnd-kit, react-markdown | Kanban drag and drop and Markdown rendering are well supported |
| Database | `rusqlite` with the `bundled` feature | No system SQLite needed on any platform |
| File watching | `notify` with a debouncer | Cross-platform file events |
| MCP | `rmcp` (the official Rust MCP SDK) on `axum`, with `tokio` | Streamable HTTP inside the app's process |
| Parsing | a frontmatter crate such as `gray_matter`, or split on `---` and parse with a YAML crate | Check crate versions and maintenance status before picking |
| Git | the git CLI, called from Rust | Only Commit review needs it; nothing extra to build, but git must be on PATH |

**Crates and modules:**

- `tandem-board-core` in `core/`, with no Tauri dependency:
  - `store`: parse and serialize task files, atomic writes, changelog append
  - `index`: SQLite connection, schema migrations, rescan
  - `service`: the one entry point for all changes (claims, transitions, edits); one instance per project, writes serialized
  - `watcher`: file events into `service`, ignoring the app's own writes
  - `git`: Commit review, which runs git add and git commit for a single task file
- `tandem-board-mcp` in `mcp/`: MCP tool handlers that call `service`
- `tandem-board-desktop` in `desktop/`: the Tauri shell, with Tauri commands for the UI and the `tasks-changed` event that `service` emits

**Self-write suppression:** after writing a file, `service` stores its hash. A watcher event whose file hash matches is ignored, so there are no write loops.

The frontend never touches files directly. In v1, closing the window quits the app and stops the MCP server.

## UI

One window: a project switcher, a five-column board, and a detail pane that slides in from the right.

**Project switcher.** The project name in the top bar is a menu: switch to another project, **New board**, **Open board** and **Remove**. With no project open, a welcome screen offers New board and Open board and lists recent projects.

- **Open board** opens a repo folder. If `.tandem/` is missing, offer to create it with its `README.md`.
- **New board** creates `.tandem/` in the folder you pick, or opens the board already there. A folder outside a git repo gets a warning first, and you choose whether to go on.
- **Remove** takes the project out of the app: its row in `app.db` and its project database. It never touches the repo or `.tandem/`, and the confirmation says so. It is refused while an agent holds a live lease on any of the project's tasks.

**Board.** Five columns: Backlog, Todo, In progress, Review, Done. Cards are sorted by priority, then created date. A card shows:

- id, title, priority dot and labels
- the parent's title as a small breadcrumb, and child progress (3/5 done) on parent cards
- a lock icon when a dependency is not done
- the agent badge with lease state: live (green dot) or stale (grey, "stale 20m")
- a blocked badge with the reason on hover
- an invalid badge for a parse error, a loop or a missing parent
- an unmet-criteria badge on Review cards, such as "2 criteria unmet", with the reasons on hover

**Drag rules:**

- Any column to any column, applying the transitions in the State model
- Dropping on Done sets `done` and `reviewed_at` in one write
- Dragging from Review back to In progress opens the Reopen dialog to ask for feedback

**Detail pane:**

- Editable title, description, acceptance criteria and relevant files (saved to the file)
- Metadata controls for priority, labels and dependencies
- Changelog timeline, read from the file
- Working-log timeline, visibly marked "local only, not committed"
- A review bar on Review cards: **Mark reviewed** and **Reopen with feedback**
- An **Approve** button on Backlog cards, which moves the task to Todo
- A **Delete task** button at the bottom, with a confirmation that lists anything blocking it
- A **Commit review** button on Done cards while their file has uncommitted changes. It commits only that task file (git add, then git commit with the file as pathspec), with the message "Review \<id>: \<title>" and a Task: \<id> trailer, and leaves anything else you have staged alone.

**Agents panel.** Connected agents, last seen, and the task each one holds.

**Setup panel.** The project's MCP URL with copy buttons for the Claude Code command, `.mcp.json` and the `AGENTS.md` snippet.

**Settings.** A page with two tabs, opened from the top bar or with `Cmd/Ctrl+,`:

- **App**, stored in `app.db`, never in a repo: the MCP server port, and the theme (system, light or dark). Changing the port restarts the server and updates every project's MCP URL; agent configs with the old URL need updating. If the new port is taken, the server keeps running on the old one.
- **Project**, for the board on screen. The name is saved in `.tandem/config.toml`, so the team shares it. Local values, never committed: the project id and MCP URL with copy buttons, **Reset local state**, with a confirmation that leases and working logs will be lost, and **Remove**, as in the project switcher.

**Also in v1:** a new-task dialog, filter by label or parent, text search, and shortcuts: `N` new task, `/` search, `Enter` open card, `R` mark reviewed, `A` approve, `Cmd/Ctrl+,` settings.

## Repo and open-source setup

One public repo on your personal GitHub, MIT licensed, that tracks its own development in its own `.tandem/`.

**Naming.** Tandem is the product name. The Rust crates use the `tandem-board-` prefix, which is free on crates.io: `tandem-board-core`, `tandem-board-mcp` and `tandem-board-desktop`. They are never published (`publish = false`), but starting with unused names avoids a rename later. The app's bundle identifier is `com.<github-user>.tandem`.

```txt
tandem/
  Cargo.toml            # workspace: core, mcp, desktop
  core/                 # tandem-board-core: store, index, service, watcher, git
  mcp/                  # tandem-board-mcp: MCP server on top of core
  desktop/              # tandem-board-desktop: Tauri shell, tauri.conf.json, commands
  ui/                   # React + Vite frontend
  site/                 # landing page: static Astro site
  .tandem/              # Tandem's own tasks, managed by Tandem
  docs/spec.md          # this spec, exported as Markdown
  scripts/              # bump-version.sh and its shared version helpers
  .github/workflows/    # ci.yml, release.yml, site-deploy.yml
  AGENTS.md  README.md  CONTRIBUTING.md  LICENSE
```

`core` and `mcp` don't depend on Tauri, so they build and test without a webview, and `mcp` can later run headless. `desktop/tauri.conf.json` points `build.frontendDist` and `build.devUrl` at `ui/`. The Tauri CLI finds the config by searching the project, so the folder doesn't need the template's `src-tauri` name; confirm this once while scaffolding.

- **CI on every pull request:** `cargo fmt --check`, `cargo clippy`, `cargo test`, and frontend lint, typecheck and tests.
- **Releases:** a tag `v*` runs `tauri-apps/tauri-action` to build macOS, Windows and Linux bundles into a draft GitHub release.
- **Versions:** the app version lives in `Cargo.toml` (`[workspace.package]`), `desktop/tauri.conf.json` and the workspace crates in `Cargo.lock`. `sh scripts/bump-version.sh X.Y.Z` (or `X.Y.Z-alpha.N`, `X.Y.Z-beta.N`) sets all three and refuses a malformed version or one that isn't higher than the current one, by SemVer precedence (`0.2.0-alpha.1 < 0.2.0-beta.1 < 0.2.0`). `scripts/version.sh` holds the version rules, shared with the release workflow's tag check.
- **Build environments:** the build fixes which environment it is, and it can't be changed at runtime or from the UI. `cargo tauri dev` is DEV, a tag `vX.Y.Z-alpha.N` is ALPHA, a tag `vX.Y.Z-beta.N` is BETA, and a tag `vX.Y.Z` (or a local release build) is production. The release workflow fails on any other tag and marks ALPHA and BETA releases as pre-releases; their Windows bundle is the NSIS installer only, since MSI refuses pre-release versions. DEV, ALPHA and BETA builds put the environment in the window title (`Tandem DEV`, `Tandem ALPHA`, `Tandem BETA`) and draw a 16px strip across the top of the window, orange, purple and blue, above every page and dialog. The strip's middle reads the environment in white, plus the version for ALPHA and BETA: `DEV build`, `ALPHA build (v0.2.0-alpha.1)`, and the page starts below it. Production shows neither.
- **Landing page:** `site/` is a static Astro site managed with bun, hosted on Vercel at `tandem.laszlomocsy.com`. Vercel's Git integration uses `site/` as the project root, and the `ignoreCommand` in `site/vercel.json` skips builds when `site/**` hasn't changed since the last deployment; pushes to `main` deploy to production and pull requests get preview deployments. Publishing a GitHub release runs `site-deploy.yml`, which builds `main` with the Vercel CLI and deploys it to production as a prebuilt deployment, so the skip can't cancel it and the download links always point at the latest release.
- **Signing:** v1 ships unsigned builds; the README explains the macOS Gatekeeper and Windows SmartScreen warnings.
- **README:** a short screen recording of an agent moving a card, quick start, agent setup, and a link to the task format in `docs/spec.md`.

## Build plan and tests

Day 1 builds the core and proves it with a real agent before any UI exists; day 2 builds the board on top.

### Day 1: core, no UI

- [ ] Scaffold the Cargo workspace (core, mcp, desktop) and the Vite + React + TypeScript UI; create the repo, license and CI skeleton
- [ ] Task file parser and serializer, with round-trip tests
- [ ] SQLite index, migrations and full rescan; project registry
- [ ] `service` with claims, leases and all transitions
- [ ] MCP server with every tool
- [ ] File watcher with self-write suppression
- [ ] End-to-end check: Claude Code starts, logs, completes and commits a task in a scratch repo

### Day 2: the app

- [ ] Board with five columns and live updates from `tasks-changed`
- [ ] Cards with lease, blocked, dependency and invalid-file badges
- [ ] Detail pane with spec editing and both timelines
- [ ] Review bar (Mark reviewed, Reopen with feedback), plus the Approve and Commit review buttons
- [ ] Drag and drop, new-task dialog, filters and shortcuts
- [ ] Setup panel with copyable configs
- [ ] README and the first tagged release

**Tests:**

| Area | Test | Passes when |
| --- | --- | --- |
| Format | Parse and re-serialize fixtures: unknown sections, CRLF input, missing optional fields | Output identical, or differs only in normalized line endings |
| Format | Append a changelog entry | Diff adds lines at the end only |
| Claims | Two MCP clients call `start_task` on the same id at once, 100 times | Exactly one wins each time |
| Leases | Lease length set to 1 minute in test config | Card turns stale; a second agent can take over |
| Rebuild | Delete the project database while the app runs | Board comes back from files; leases are gone |
| Git | Switch to a branch with different statuses | Board updates; leases for tasks not `in_progress` are dropped |
| Watcher | Edit a task in another editor | Board updates, no write loop |
| Invalid file | Break a file's YAML | Card shows the error; the file is not overwritten |
| Dogfood | Two agents on two tasks in one checkout; you review both | Both reach Done with clean commits |
| Graph | Hand-edit files into a parent loop and a dependency loop; then try the same through MCP | Hand-edited tasks get the invalid badge; the MCP calls return cycle\_detected |
| Rebuild | Edit a task file while the app is closed, then open it | The startup scan picks up the change |
| Format | Set format\_version higher than the app supports | Project opens read-only with an update prompt |
| States | An agent creates a task, then calls start\_task on it | Refused with invalid\_transition until you approve it into Todo |
| Git | Press Commit review while other changes are staged | The new commit holds only the task file; everything else stays staged |
| Criteria | finish\_task leaves one criterion neither met nor explained; then retry with a reason | First call refused with criteria\_unmet; the retry passes and the Review card shows the badge |

## Scope and later versions

**Later versions:**

- **Worktrees.** Parallel agents often run in separate git worktrees, each with its own copy of `.tandem/`. Planned fix: identify a project by its git common dir, and add a `cwd` argument so writes go to the agent's worktree.
- **Local security.** Any local process can call the MCP server. Planned fix: a per-project bearer token in the generated config.

**Out of scope for v1:** multiple users or sync, notifications, launching agents from the app, git operations other than Commit review, multiple windows, and keeping the MCP server running in the tray.

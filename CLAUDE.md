# Tandem

Tandem is a local Tauri desktop app with a kanban board for tasks that you and AI coding agents share. Tasks live in a SQLite database per project in the app's data directory, backed up next to it; nothing is written into the repo. A project is identified by its repo's git common dir, so every worktree shares one board. Agents claim, work on and finish tasks through an MCP server built into the app, and a human reviews and ticks off what they finish. A Rust core owns every database change; the UI and the MCP server both go through it.

## Layout

```txt
Cargo.toml       # workspace: core, mcp, desktop, xtask
core/            # tandem-board-core: model, db (schema, migrations, backups), service, graph, git, markdown
mcp/             # tandem-board-mcp: MCP tool handlers on top of core
desktop/         # tandem-board-desktop: Tauri 2 shell, tauri.conf.json, commands
xtask/           # repo tooling, run as `cargo xtask`: version checks and bumps
ui/              # React + TypeScript + Vite frontend, managed with bun
site/            # landing page: static Astro site, managed with bun, on Vercel
.github/workflows/ci.yml
```

## Build, test and run

Rust, from the repo root:

```sh
cargo build --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Frontend, from `ui/`:

```sh
bun install
bun run lint        # oxlint
bun run typecheck   # tsc -b
bun run test        # vitest
```

Version tooling, from anywhere in the repo (see CONTRIBUTING.md for the release flow):

```sh
cargo xtask version                       # list every command and option
cargo xtask version current               # print the app version
cargo xtask version --next minor --dry-run   # preview a change; --next/--exact set it in all three files
cargo xtask version check pr              # the checks CI runs on this branch's pull request
```

App, from the repo root (needs the Tauri CLI: `cargo install tauri-cli --version "^2"`):

```sh
cargo tauri dev     # starts Vite in ui/ and the app with hot reload
cargo tauri build   # release bundle
```

CI runs the fmt, clippy, test, lint, typecheck and frontend test commands above on every pull request and every push to `main`.

## Rules

- `core` and `mcp` never depend on Tauri, directly or through another crate. Tauri code lives only in `desktop`.
- Every change to `core` comes with tests.
- The database schema is the migrations in `core/src/db/schema.rs`: each list, read in order, builds the current schema from nothing. Change the schema only by appending a migration; never edit one that has shipped.
- Every service call runs in one `BEGIN IMMEDIATE` transaction, so checks and changes land together. Restoring a backup is the one exception: it marks the database as restoring, which refuses every other call in any process, backs up the current state, then replaces the whole database in one SQLite write that clears the mark.
- A task's events are append-only. Deleting a task sets `deleted_at`; only cleaning up deleted tasks removes rows, after the user confirms. Restoring a backup, which the user also confirms, brings back the backup's rows as they were.
- `core/src/markdown/` is the Markdown task format: how `get_task` shows a task, and how an old `.tandem/` folder is imported. The app never writes it into a repo.

# Tandem

Tandem is a local Tauri desktop app with a kanban board for tasks that you and AI coding agents share. Each task is a Markdown file with YAML frontmatter in the repo's `.tandem/` folder, committed with the code that works on it. Agents claim, work on and finish tasks through an MCP server built into the app, and a human reviews and ticks off what they finish. A Rust core owns every file write and database change; the UI and the MCP server both go through it. SQLite holds in-flight activity only and can always be rebuilt from the files.

## Layout

```txt
Cargo.toml       # workspace: core, mcp, desktop
core/            # tandem-board-core: store, index, service, watcher, git
mcp/             # tandem-board-mcp: MCP tool handlers on top of core
desktop/         # tandem-board-desktop: Tauri 2 shell, tauri.conf.json, commands
ui/              # React + TypeScript + Vite frontend, managed with bun
docs/spec.md     # the v1 spec
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

App, from the repo root (needs the Tauri CLI: `cargo install tauri-cli --version "^2"`):

```sh
cargo tauri dev     # starts Vite in ui/ and the app with hot reload
cargo tauri build   # release bundle
```

CI runs the fmt, clippy, test, lint, typecheck and frontend test commands above on every pull request and every push to `main`.

## Rules

- `docs/spec.md` is the source of truth. Ask before deviating from it.
- `core` and `mcp` never depend on Tauri, directly or through another crate. Tauri code lives only in `desktop`.
- Every change to `core` comes with tests.
- Task-file writes keep diffs minimal, as the spec's write rules describe:
  - Frontmatter is written in the spec's fixed key order, and empty optional fields are omitted.
  - Touch only the lines the app owns. Keep the body byte for byte, including unknown sections, except for the changelog append.
  - The changelog is append-only, oldest entry first, so diffs only add lines at the end.
  - LF line endings and exactly one trailing newline.
  - Write atomically: a temp file in `.tandem/`, then rename.
  - If a file fails to parse, mark it invalid and never overwrite it.

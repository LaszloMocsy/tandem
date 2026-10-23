# Tandem

A task board that you and your AI coding agents share.

Tasks live in the app, on your machine; nothing is added to your repo. Agents claim tasks, log progress and finish them through an MCP server built into the app. You approve what they propose, review what they finish, and watch it all happen on a five-column board: Backlog, Todo, In progress, Review and Done.

## How it works

- **Tasks live in the app.** Each project has its own SQLite database in the app's data folder, holding every task with its acceptance criteria and full history: the changelog and the agents' working notes, kept for good. Your repo stays untouched.
- **One board per repo.** A project is identified by its repo, so every git worktree of it opens the same board, and agents in parallel worktrees share it.
- **Agents work through MCP.** An agent calls `start_task` to claim a task, `log_progress` and `set_blocked` while it works, and `finish_task` when it's done. Claims are leases, so a crashed agent's task can be taken over. Agents add a `Task: <id>` trailer to their commits, so git still links the code to the task.
- **You stay in charge.** Tasks an agent proposes wait in Backlog until you approve them, and only you can mark finished work reviewed. Deleted tasks can be restored from Settings until you clean them up.

## Quick start

1. Download the build for your platform from the [latest release](https://github.com/LaszloMocsy/tandem/releases/latest). The builds aren't signed yet, so see [Installing an unsigned build](#installing-an-unsigned-build) before you open it.
2. Open Tandem, choose **Open folder** and pick a git repo, or any folder in it. A folder outside git works too, after a warning. If the repo has a `.tandem/` folder from an earlier version, its tasks are imported the first time you open it; the folder is only read, and you can delete it afterwards.
3. Press **N** to create a task. Yours start in Todo, ready for an agent.
4. Connect an agent, as described in [Connecting an agent](#connecting-an-agent), and ask it to work on the task.
5. Watch the card move to In progress with the agent's badge, then to Review. Open it, check the work, and press **Mark reviewed**, or **Reopen with feedback** to send it back.

Shortcuts: **N** new task, **/** search, **Enter** open the focused card, **R** mark reviewed, **A** approve. You can also drag cards between columns; each move applies the matching step, such as approving a task or marking it reviewed.

## Connecting an agent

Tandem serves MCP over HTTP on `127.0.0.1:4317`, with one endpoint per project, such as `http://127.0.0.1:4317/p/<project-id>/mcp`. It listens on localhost only, and only while the app is running.

The **Setup** panel shows your project's URL, with copy buttons for each of these:

- **Claude Code:** run this in the repo.

  ```sh
  claude mcp add --transport http tandem http://127.0.0.1:4317/p/<project-id>/mcp
  ```

- **`.mcp.json`:** or commit this at the repo root, so every Claude Code session in the repo gets the server.

  ```json
  {
    "mcpServers": {
      "tandem": { "type": "http", "url": "http://127.0.0.1:4317/p/<project-id>/mcp" }
    }
  }
  ```

- **`AGENTS.md` snippet:** add it to `AGENTS.md` or `CLAUDE.md`, so agents claim tasks before working, tag their commits with the task and never mark their own work reviewed:

  ```markdown
  ## Tasks

  This repo's tasks live in the Tandem app; agents work on them through the `tandem` MCP server.

  - Before starting work, call `start_task` on the task (use `next_task` if none was given), and pass the agent id it returns as `agent` to every later tandem call for that task.
  - Add a `Task: <id>` trailer to every commit for that task.
  - Use `log_progress` for notes and `set_blocked` when you need a decision from me.
  - Split large work into child tasks with `create_task` and `parent`. New tasks wait in backlog until I approve them; only start tasks in todo.
  - Call `finish_task` with a short summary when the work is done. Tick each acceptance criterion you met and explain each one you didn't. Never mark tasks reviewed.
  - If the tandem server is unreachable, ask me to open the app.
  ```

Other MCP clients that support Streamable HTTP can use the same URL; see your client's docs for its config format.

The **Agents** panel lists the agents that have called recently and the task each one holds.

## Installing an unsigned build

The first releases aren't signed or notarized, so macOS and Windows warn before the first launch.

**macOS.** Use the `.dmg` for your Mac: `aarch64` for Apple silicon, `x64` for Intel. Drag Tandem to Applications and open it. macOS says it can't verify the app. Close the message, open **System Settings → Privacy & Security**, and under Security choose **Open Anyway** for Tandem, then confirm. You only do this once.

On Apple silicon, macOS may instead say Tandem "is damaged and can't be opened". The app is fine; macOS says this about unsigned apps downloaded from the internet. Clear the download quarantine in Terminal, then open Tandem again:

```sh
xattr -dr com.apple.quarantine /Applications/Tandem.app
```

**Windows.** Run the `-setup.exe` or the `.msi`. If SmartScreen shows "Windows protected your PC", choose **More info**, then **Run anyway**.

**Linux.** Install the `.deb` (`sudo apt install ./Tandem_*.deb`) or the `.rpm`, or make the `.AppImage` executable (`chmod +x Tandem_*.AppImage`) and run it.

Only download Tandem from this repo's [releases](https://github.com/LaszloMocsy/tandem/releases).

## Building from source

You need [Rust](https://rustup.rs), [Bun](https://bun.sh), the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform and the Tauri CLI (`cargo install tauri-cli --version "^2"`).

```sh
bun install --cwd ui   # frontend dependencies
cargo tauri dev        # run the app with hot reload
cargo tauri build      # build a release bundle
```

Checks, which CI runs on every pull request and every push to `main`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cd ui
bun run lint        # oxlint
bun run typecheck   # tsc -b
bun run test        # vitest
```

## Repo layout

| Path | What it is |
| --- | --- |
| `core/` | `tandem-board-core`: the task model, the SQLite databases and their migrations, the task service, the Markdown task format, git |
| `mcp/` | `tandem-board-mcp`: MCP server on top of core |
| `desktop/` | `tandem-board-desktop`: the Tauri 2 shell and `tauri.conf.json` |
| `ui/` | React + TypeScript + Vite frontend |
| `site/` | The landing page, a static Astro site |

`core` and `mcp` don't depend on Tauri, so they build and test without a webview.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE)

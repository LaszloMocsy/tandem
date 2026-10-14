# Tandem

A task board that you and your AI agents share.

Tandem is in early development. The design, including the task file format, is in [docs/spec.md](docs/spec.md).

## Repo layout

| Path | What it is |
| --- | --- |
| `core/` | `tandem-board-core`: task store, SQLite index, task service, file watcher, git |
| `mcp/` | `tandem-board-mcp`: MCP server on top of core |
| `desktop/` | `tandem-board-desktop`: the Tauri 2 shell and `tauri.conf.json` |
| `ui/` | React + TypeScript + Vite frontend |

`core` and `mcp` don't depend on Tauri, so they build and test without a webview.

## Development

You need [Rust](https://rustup.rs), [Bun](https://bun.sh), the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform and the Tauri CLI (`cargo install tauri-cli --version "^2"`).

```sh
bun install --cwd ui   # frontend dependencies
cargo tauri dev        # run the app with hot reload
cargo tauri build      # build a release bundle
cargo test             # Rust tests
```

## License

[MIT](LICENSE)

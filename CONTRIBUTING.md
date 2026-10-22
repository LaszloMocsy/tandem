# Contributing to Tandem

Thanks for helping. Small, focused pull requests are the easiest to review; for anything larger, open an issue first so we can agree on the approach.

## Start from the spec

[docs/spec.md](docs/spec.md) is the source of truth for how Tandem behaves: the task file format, the states and transitions, the MCP tools and the UI. If your change needs to differ from it, update the spec in the same pull request and say why, or open an issue to discuss it first.

## Setting up

Follow [Building from source](README.md#building-from-source) in the README: you need Rust, Bun, the Tauri prerequisites for your platform and the Tauri CLI. Then:

```sh
bun install --cwd ui
cargo tauri dev
```

The app keeps its project list and per-project databases in its data folder, such as `~/Library/Application Support/com.laszlomocsy.tandem` on macOS. To try changes without touching your real projects, use a scratch git repo.

## Rules the code follows

- `core` and `mcp` never depend on Tauri, directly or through another crate. Tauri code lives only in `desktop`.
- Every change to `core` comes with tests.
- Every file write and database change goes through the task service in `core`. The UI and the MCP server never write files themselves.
- Task-file writes keep diffs minimal:
  - Frontmatter is written in the spec's fixed key order, and empty optional fields are left out.
  - Only the lines the app owns change. The body is kept byte for byte, including unknown sections, except for changelog appends.
  - The changelog is append-only, oldest entry first, so diffs only add lines at the end.
  - Files use LF line endings and end in exactly one newline.
  - Writes are atomic: a temp file in `.tandem/`, then a rename.
  - A file that fails to parse is marked invalid and never overwritten.
- The UI uses plain CSS. Please discuss new dependencies, especially component libraries, in an issue first.

## Tests and checks

Run these before opening a pull request. CI runs the same ones on every pull request and every push to `main`.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cd ui
bun run lint        # oxlint
bun run typecheck   # tsc -b
bun run test        # vitest
```

Some Rust tests run the git CLI in temporary repos, so git must be on your `PATH`; they turn off commit signing and hooks so your own git config doesn't affect them. The file-watcher tests wait for real file events and take a few seconds.

Changes to the board should also be tried in the running app with `cargo tauri dev`. The spec's [Build plan and tests](docs/spec.md#build-plan-and-tests) lists the checks that still need a human.

## Commits and pull requests

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org) with the crate or area as the scope, such as `feat(core): …`, `fix(ui): …`, `test: …` or `docs: …`. The body says what changed and why.
- Keep each commit to one topic, with its tests in the same commit.
- When you work on a Tandem task, add a `Task: <id>` trailer to every commit for it.

## Releasing

For maintainers:

1. Set the new version with `cargo xtask version bump 0.2.0` (or `0.2.0-alpha.1`, `0.2.0-beta.1`). It updates `Cargo.toml` (`[workspace.package]`), `desktop/tauri.conf.json` and `Cargo.lock`, and refuses a version that isn't higher than the current one. Commit the result.
2. Tag the commit and push the tag:

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. The Release workflow checks that the tag matches the version (`cargo xtask version check --tag`), then builds macOS (Apple silicon and Intel), Windows and Linux bundles into a draft GitHub release.
4. Check the assets, edit the notes, and publish the draft.

The builds are not signed yet; [Installing an unsigned build](README.md#installing-an-unsigned-build) explains what users see.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).

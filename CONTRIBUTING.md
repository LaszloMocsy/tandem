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

## Branches

Feature branches merge into a `version/vX.Y.Z` branch, and only version branches merge into `main`. Branch your feature from the version branch it will go into, and open its pull request against that branch. Every release, pre-releases included, gets its own version branch named after its exact version, such as `version/v0.2.0-alpha.1`. The Version workflow checks every pull request into `main`: it must come from a `version/vX.Y.Z` branch whose name matches the version in `Cargo.toml`, `desktop/tauri.conf.json` and `Cargo.lock`, and that version must be higher than `main`'s. Run the same check locally with:

```sh
cargo xtask version check --base origin/main --branch "$(git branch --show-current)"
```

For maintainers, once per repository:

- Make **Version** a required status check for `main` (Settings → Branches → branch protection), ideally with "Require branches to be up to date before merging", so the comparison is against the latest `main`. Until then the check reports but doesn't block.
- Create the `skip-release-notes` label, which keeps version pull requests out of the release notes (see Releasing).

## Releasing

For maintainers:

1. Create `version/vX.Y.Z` from `main` and set the version with `cargo xtask version bump X.Y.Z` (or `X.Y.Z-alpha.N`, `X.Y.Z-beta.N`). It updates `Cargo.toml` (`[workspace.package]`), `desktop/tauri.conf.json` and `Cargo.lock`, and refuses a version that isn't higher than the current one. Commit the result.
2. Merge the release's feature branches into the version branch, then open a pull request from it into `main`.
3. Merging that pull request runs the Tag workflow: it tags the merge commit `vX.Y.Z` and starts the Release workflow for the tag. It refuses a tag that already exists.
4. The Release workflow checks that the tag matches the version (`cargo xtask version check --tag`), then builds macOS (Apple silicon and Intel), Windows and Linux bundles into a draft GitHub release.
5. Check the assets, edit the notes, and publish the draft.

The draft's notes list the pull requests merged since the previous release: since the last production release for a production version, so they cover its alpha and beta releases too, or since the last release of any kind for a pre-release. `.github/release.yml` groups them by label: `enhancement` under New features, `bug` under Fixes, `documentation` under Documentation, and the rest under Other changes. So that the feature pull requests show up:

- Label feature pull requests as you merge them into the version branch.
- Merge the version pull request into `main` with a merge commit, not a squash, which would hide the feature pull requests' commits.
- Label the version pull request `skip-release-notes`, since its feature pull requests are already listed.

To retry a failed release, delete its draft and run the Release workflow on the tag again: `gh workflow run release.yml --ref vX.Y.Z`. Pushing a new `v*` tag by hand also starts a release.

The builds are not signed yet; [Installing an unsigned build](README.md#installing-an-unsigned-build) explains what users see.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).

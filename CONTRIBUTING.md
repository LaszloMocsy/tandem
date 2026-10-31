# Contributing to Tandem

Thanks for helping. Small, focused pull requests are the easiest to review; for anything larger, open an issue first so we can agree on the approach.

## Setting up

Follow [Building from source](README.md#building-from-source) in the README: you need Rust, Bun, the Tauri prerequisites for your platform and the Tauri CLI. Then:

```sh
bun install --cwd ui
cargo tauri dev
```

The app keeps its project list and every project's tasks in databases in its data folder, such as `~/Library/Application Support/com.laszlomocsy.tandem/db` on macOS. To try changes without touching your real boards, point the app at another folder with `TANDEM_DATA_DIR`, as in `TANDEM_DATA_DIR=/tmp/tandem cargo tauri dev`, or use a scratch git repo and remove its project afterwards.

## Rules the code follows

- `core` and `mcp` never depend on Tauri, directly or through another crate. Tauri code lives only in `desktop`.
- Every change to `core` comes with tests.
- Every database change goes through the task service in `core`, one transaction per call. The UI and the MCP server never touch the databases themselves.
- The schema is the migrations in `core/src/db/schema.rs`. Change it only by appending a migration, never by editing one that has shipped.
- A task's events are append-only. Deleted tasks keep their rows until the user cleans them up.
- The app never writes into a repo.
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

Some Rust tests run the git CLI in temporary repos, so git must be on your `PATH`; they turn off commit signing and hooks so your own git config doesn't affect them.

Changes to the board should also be tried in the running app with `cargo tauri dev`.

## Commits and pull requests

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org) with the crate or area as the scope, such as `feat(core): …`, `fix(ui): …`, `test: …` or `docs: …`. The body says what changed and why.
- Keep each commit to one topic, with its tests in the same commit.
- When you work on a Tandem task, add a `Task: <id>` trailer to every commit for it.

## Branches

Feature branches merge into a `release/vX.Y.Z` branch, and only release branches merge into `main`. Branch your feature from the release branch it will go into, and open its pull request against that branch. A release branch is named after the stable version it becomes, such as `release/v0.2.0`, and its `Cargo.toml`, `desktop/tauri.conf.json` and `Cargo.lock` hold that version. Its betas are built from the same branch and don't change the version files (see Releasing). The Version workflow checks every pull request into `main`: it must come from a `release/vX.Y.Z` branch whose name matches the version in those files, and that version must be higher than `main`'s. A feature pull request into a release branch must keep that branch's version. Run the same check locally for your branch's pull request, or any other by number (it uses `gh`):

```sh
cargo xtask version check pr
cargo xtask version check pr 12
```

For maintainers, once per repository:

- Make **Version** a required status check for `main` (Settings → Branches → branch protection), ideally with "Require branches to be up to date before merging", so the comparison is against the latest `main`. Until then the check reports but doesn't block.
- Create the `skip-release-notes` label, which keeps release pull requests out of the release notes (see Releasing).

## Releasing

Tandem has three builds:

- **Dev**: `cargo tauri dev`, and any `cargo tauri build` on your machine. Labelled DEV, and never updates itself.
- **Beta** `vX.Y.Z-beta.N`: built when you run the Beta workflow on a release branch. Labelled BETA, published as a GitHub pre-release, and offered on the beta update channel.
- **Stable** `vX.Y.Z`: built when a release branch merges into `main`. Offered on both channels.

The version files only ever hold `X.Y.Z`. A beta's version exists only in its tag and its build, because some bundlers refuse a `-` in the version.

For maintainers:

1. Create the release branch and set the version in one step, then commit the result and push it:

   ```sh
   cargo xtask version --next minor --create-branch   # 0.1.0 -> 0.2.0 on release/v0.2.0
   cargo xtask version --next patch --create-branch   # 0.2.0 -> 0.2.1 on release/v0.2.1
   ```

   It updates `Cargo.toml` (`[workspace.package]`), `desktop/tauri.conf.json` and `Cargo.lock`. Add `--dry-run` to see the change first, or use `--exact X.Y.Z` to set a version directly; lowering one asks first. `cargo xtask version` lists every option.
2. Merge the release's feature branches into the release branch.
3. For a beta, run the Beta workflow on the release branch, from Actions → Beta → Run workflow, or:

   ```sh
   gh workflow run beta.yml --ref release/v0.2.0
   ```

   It tags the branch's head `v0.2.0-beta.N`, one past the highest beta of `0.2.0` tagged so far (drafts included), and starts the Release workflow for that tag. `cargo xtask version next-beta` prints the tag it would pick for your branch. Run it again after more changes for the next beta; the version files stay `0.2.0` throughout. Once `v0.2.0` is out, its betas are over, and the next ones need a new release branch.
4. When the release is ready, open a pull request from the release branch into `main`. Merging it runs the Tag workflow: it tags the merge commit `vX.Y.Z` and starts the Release workflow for the tag. It refuses a tag that already exists.
5. The Release workflow checks that the version files hold `X.Y.Z` for the tag `vX.Y.Z` or `vX.Y.Z-beta.N` (`cargo xtask version check release`). Each build then sets the files in its own checkout to the tag's version (`cargo xtask version stamp`), so a beta carries `X.Y.Z-beta.N`, and builds macOS (Apple silicon and Intel), Windows and Linux bundles into a draft GitHub release.
   - A stable release gets every bundle: `.dmg`, the Windows NSIS installer and `.msi`, and the Linux `.AppImage`, `.deb` and `.rpm`.
   - A beta gets the `.dmg`, the NSIS installer and the `.AppImage`. The MSI and RPM bundlers refuse `-beta.N`, and dpkg would sort a beta's `.deb` above its stable release, so those are left out. The three a beta keeps are the ones the updater installs, so betas update on every platform.
6. Check the assets, edit the notes, and publish the draft. Publishing also ships the update: it redeploys the landing page, whose update feeds (`/updates/stable.json` and `/updates/beta.json`) then offer the release to every app on its channel.

The draft's notes list the pull requests merged since the previous release: since the last stable release for a stable version, so they cover its betas too, or since the last release of any kind for a beta. `.github/release.yml` groups them by label: `enhancement` under New features, `bug` under Fixes, `documentation` under Documentation, and the rest under Other changes. So that the feature pull requests show up:

- Label feature pull requests as you merge them into the release branch.
- Merge the release pull request into `main` with a merge commit, not a squash, which would hide the feature pull requests' commits and leave the betas' tags off `main`.
- Label the release pull request `skip-release-notes`, since its feature pull requests are already listed.

To retry a failed release, delete its draft and run the Release workflow on the tag again: `gh workflow run release.yml --ref vX.Y.Z`, or `--ref vX.Y.Z-beta.N` for a beta. Pushing a new `v*` tag by hand also starts a release.

Edit the notes below the `<!-- changes -->` line, and keep the line: the app's update dialog shows only what follows it.

Updates are signed with the updater key in the `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets, and the app only installs updates signed by it; its public key is in `desktop/tauri.conf.json`. Keep a copy of the key and its password outside GitHub: if they're lost, installed apps can never update again. The key is separate from OS code signing.

Release bundles need the key, so `cargo tauri build` on your machine fails at signing without it. Build with `cargo tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'` to skip the update bundles.

A build on your machine is a dev build: labelled DEV, and it never updates itself. The Release workflow sets `TANDEM_ENV` to `beta` or `production` for its builds; set it yourself, as in `TANDEM_ENV=production cargo tauri build`, to try a build labelled like a release. Any other value fails the build.

The builds are not signed yet; [Installing an unsigned build](README.md#installing-an-unsigned-build) explains what users see.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).

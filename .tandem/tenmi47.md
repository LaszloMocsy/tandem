---
id: tenmi47
title: Replace the version shell scripts with cargo xtask
status: done
priority: high
parent: b85qh8f
labels: [area:release, feat/versioning]
created: 2026-10-22
reviewed_at: 2026-10-22
---

## Description
Move the version tooling into an internal Rust CLI that workflows and local development share, so the logic is tested by `cargo test --workspace` and works on Windows too.

- New workspace member `xtask/` (`publish = false`, no Tauri dependency), run through a `.cargo/config.toml` alias: `cargo xtask …`. Parse arguments by hand (no clap) to keep the first build short.
- Commands:
  - `cargo xtask version`: print the current version.
  - `cargo xtask version check`: the version in `Cargo.toml`, `desktop/tauri.conf.json` and `Cargo.lock` agrees.
  - `cargo xtask version bump <X.Y.Z[-alpha.N|-beta.N]>`: same behaviour as `scripts/bump-version.sh` today (validation, must be higher, clean files, one-line edits, `cargo update --workspace --offline` for `Cargo.lock`, branch warning, next steps).
  - `cargo xtask version env <tag>`: replaces `.github/scripts/build-env.sh` (prints alpha, beta or nothing; fails on any other tag).
- Edit `Cargo.toml` with `toml_edit` (already a dependency of core) so formatting is kept. Edit `tauri.conf.json` by replacing the version line only.
- Version parsing and SemVer comparison have unit tests, including the cases from the old build-env test.
- Switch `release.yml` to `cargo xtask version env` and `cargo xtask version check` against the tag. The `create-release` job needs the Rust toolchain; keep the "no build cache" rule.
- Delete `scripts/version.sh`, `scripts/bump-version.sh` and `.github/scripts/build-env.sh`. Update the spec's Versions bullet and layout.
- Later commands (`check --base`, `previous-tag`) come with the tasks that need them.

CI: xtask is a workspace member, so the existing Rust job runs its fmt, clippy and tests; no separate workflow for the tool itself.

## Acceptance criteria
- [ ] cargo xtask version, version check, version bump and version env work locally on macOS, Linux and Windows
- [x] Version parsing, comparison and file edits are covered by unit tests run by cargo test --workspace
- [x] release.yml uses the xtask commands and still rejects unsupported tags and tags that don't match the app version
- [x] The shell scripts are removed and the spec describes the xtask commands
- [x] A bump still changes only the version lines in Cargo.toml, tauri.conf.json and Cargo.lock, and cargo build --workspace --locked passes after it

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-4459 · started

### 2026-10-22 · claude-code-4459 · finished
Added the `xtask` workspace crate, run as `cargo xtask` through a `.cargo/config.toml` alias. Commands: `version` (print), `version check [--tag vX.Y.Z]` (Cargo.toml, tauri.conf.json and every workspace crate in Cargo.lock agree, and match the tag), `version bump X.Y.Z[-alpha.N|-beta.N]` (same rules as the shell script) and `version env <tag>`. Cargo.toml is edited with toml_edit, keeping decor. tauri.conf.json changes only its top-level version line. Both edits must change exactly one line, files are written through a temp file and rename, and Cargo.lock is refreshed with `cargo update --workspace --offline`. xtask has its own version 0.0.0, so a bump doesn't touch it. There are 9 unit tests for parsing, the old build-env cases, SemVer ordering and the file edits. release.yml's create-release job now installs Rust (pinned SHA, no cache) and uses `cargo --locked xtask version env` and `version check --tag`. The three shell scripts are deleted. The spec, CLAUDE.md and CONTRIBUTING.md describe the commands. Checked on macOS: every command, the refusals, and a real bump in a throwaway worktree (only the five version lines changed and `cargo build --workspace --locked` passed). Full fmt, clippy and test pass.
Files: xtask/Cargo.toml, xtask/src/main.rs, xtask/src/version.rs, xtask/src/files.rs, .cargo/config.toml, Cargo.toml, Cargo.lock, .github/workflows/release.yml, scripts/version.sh, scripts/bump-version.sh, .github/scripts/build-env.sh, docs/spec.md, CLAUDE.md, CONTRIBUTING.md
Unmet: 1 (Verified on macOS only. Linux runs it in the release workflow's create-release job; Windows is untested. The code avoids shells and platform-specific paths (cargo via $CARGO, git via Command, std::fs::rename replaces on Windows, line matching tolerates CRLF), so I expect it to work.)

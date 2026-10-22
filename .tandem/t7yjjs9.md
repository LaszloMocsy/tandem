---
id: t7yjjs9
title: Redesign the cargo xtask version CLI with clap
status: finished
priority: medium
labels: [area:release, feat/versioning]
created: 2026-10-22
---

## Description
Rework `cargo xtask version` around how it's actually used, with `clap` for parsing, help and errors. Decided with the user on 2026-10-22.

### Commands

```sh
cargo xtask version                         # prints the help: every command and flag
cargo xtask version current                 # prints the app version
cargo xtask version --next <level> [--pre alpha|beta] [--dry-run] [--create-branch]
cargo xtask version --exact <version> [--yes] [--dry-run] [--create-branch]
cargo xtask version check                   # the version files agree; prints the version
cargo xtask version check pr [N]            # the PR rules, for the current branch's PR or PR #N
cargo xtask version check release [TAG]     # the tag on HEAD (or TAG) names the version
cargo xtask version check --against <rev>   # higher than the version on any git rev
cargo xtask version env <tag>               # unchanged; used by release.yml
cargo xtask version previous-tag <tag>      # unchanged; used by release.yml
```

### Bumping
- `--next major|minor|patch` from a production version: `0.1.0` → `1.0.0` / `0.2.0` / `0.1.1`. With `--pre alpha|beta`: `--next minor --pre alpha` → `0.2.0-alpha.1`.
- `--next alpha|beta` on a pre-release: `0.2.0-alpha.1` → `0.2.0-alpha.2` (alpha) or `0.2.0-beta.1` (beta). Refused if it would go lower (`beta.2` → alpha) or from a production version without a level (ambiguous).
- `--next release` drops the suffix: `0.2.0-beta.2` → `0.2.0`. Refused on a production version.
- `--exact <version>` sets any valid version. A lower version asks for y/N confirmation on a terminal, needs `--yes` otherwise, and is refused without a terminal or `--yes`. The same version is an error.
- `--dry-run` prints `0.1.0 -> 0.2.0` and the files it would change, and writes nothing.
- `--create-branch` cuts `version/v<new>` from `main` (after `git fetch`) and bumps on it. It refuses if the branch exists or the working tree isn't clean.
- Keep today's safety rules: the files must agree, the three version files must have no uncommitted changes, each edit must change exactly one line, and `Cargo.lock` is refreshed with `cargo update --workspace --offline`. Print the next steps.

### Checks
- `check` only checks that the version files agree.
- `check pr [N]` resolves the PR: in GitHub Actions from `GITHUB_HEAD_REF`, `GITHUB_BASE_REF` and the event payload; locally through `gh pr view [N] --json number,headRefName,baseRefName,state`. It fetches the base, then by base:
  - into `main`: the head is `version/vX.Y.Z` matching the version files, and the version is higher than the base's. When the PR is already merged (`tag.yml`), only the naming and consistency apply.
  - into `version/vX.Y.Z`: the head's version equals the version branch's, since feature branches don't bump.
  - into anything else: consistency only.
- `check release [TAG]`: TAG, or else `GITHUB_REF_NAME` in Actions, or else the tag on HEAD, must be `v<version>`.
- `check --against <rev>`: the version is higher than `Cargo.toml`'s on any git rev (branch, tag or SHA).
- `--branch`, `--tag`, `--base` and `version bump` go away.

### Also
- Workflows move to the new commands: `version.yml` → `version check pr`; `tag.yml` → `version check pr` (merged), reading the version from its output or `version current`; `release.yml` → `version check release`.
- Errors say what to run next. Unit tests cover version arithmetic, the PR rules per base, and option conflicts (clap's `debug_assert` too).
- Update the spec, CONTRIBUTING.md, CLAUDE.md, the PR template and the printed next steps.

## Acceptance criteria
- [x] `cargo xtask version` prints the help, and `version current` prints the version
- [x] `--next major|minor|patch|alpha|beta|release` with `--pre` gives the versions in the description and refuses ambiguous or lower results
- [x] `--exact` asks before lowering on a terminal, needs `--yes` otherwise, and refuses the same version
- [x] `--dry-run` writes nothing, and `--create-branch` cuts version/v<new> from main
- [ ] `check`, `check pr [N]`, `check release [TAG]` and `check --against <rev>` enforce the rules in the description, locally and in GitHub Actions
- [x] version.yml, tag.yml and release.yml use the new commands, and the old flags and `bump` are gone
- [x] Version arithmetic, the PR rules and option parsing are covered by unit tests
- [x] The spec, CONTRIBUTING.md, CLAUDE.md and the PR template describe the new CLI

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-1bdf · started

### 2026-10-22 · claude-code-1bdf · finished
Rebuilt `cargo xtask version` on clap 4. New modules: cli.rs (definitions), change.rs (--next/--exact), repo.rs (files, git, gh); check.rs is rewritten. `version` prints the help with examples, and `version current` prints the version. `--next major|minor|patch|alpha|beta|release` comes with `--pre alpha|beta` (Version::next refuses ambiguous, lower or overflowing results), and `--exact` accepts a leading v. Lowering asks y/N on a terminal and needs `--yes` otherwise; the same version is an error. `--dry-run` writes nothing. `--create-branch` fetches main, computes the new version from main's, refuses an existing branch (local or origin) or a dirty tree, and cuts version/v<new>. `check`, `check --against REV`, `check pr [N]` and `check release [TAG]` work as specified. `check pr` reads Actions env plus the event payload, or gh, and fetches the PR head and base by full ref, so a branch name can't act as a git option. Every check prints the version. `bump` and `--branch/--tag/--base` are gone, while `env` and `previous-tag` are kept. Workflows: version.yml runs `version check pr` (no separate fetch step or branch env), tag.yml runs `version check pr` on the merged PR, and release.yml runs `version check release "$TAG"`. There are 25 xtask tests: version arithmetic, PR rules per base, release-tag lookup, and clap parsing/conflicts with `debug_assert`. Checked end to end in a throwaway worktree: every --next path and refusal; --exact refusal, --yes, and the real y/N prompt through `expect`; --dry-run; real --create-branch and the exists refusal; check release; check pr in a simulated Actions env (open and merged), against real PRs #20 (refused: not a version branch) and #5 (feature → feature, passes), and with no PR. Spec, CONTRIBUTING, CLAUDE.md and the PR template updated. The full fmt, clippy and test suite passes.
Files: xtask/Cargo.toml, Cargo.lock, xtask/src/cli.rs, xtask/src/change.rs, xtask/src/repo.rs, xtask/src/check.rs, xtask/src/main.rs, xtask/src/version.rs, .github/workflows/version.yml, .github/workflows/tag.yml, .github/workflows/release.yml, .github/pull_request_template.md, docs/spec.md, CONTRIBUTING.md, CLAUDE.md
Unmet: 5 (Verified locally, with GitHub Actions simulated through its environment variables and event payload, but not yet run on GitHub: version.yml, tag.yml and release.yml first run with the new commands after this branch is pushed.)

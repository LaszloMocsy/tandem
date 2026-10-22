---
id: 46zgra7
title: Version bump script
status: done
priority: high
parent: b85qh8f
labels: [area:release, feat/versioning]
created: 2026-10-22
reviewed_at: 2026-10-22
---

## Description
`scripts/bump-version.sh <X.Y.Z | X.Y.Z-alpha.N | X.Y.Z-beta.N>`, run by hand on a new `version/vX.Y.Z` branch.

- Validates the version with the same rules as `.github/scripts/build-env.sh`. Share the regex rather than copy it.
- Refuses a version that isn't higher than the current one (SemVer precedence: `0.2.0-alpha.1 < 0.2.0-beta.1 < 0.2.0`).
- Updates `[workspace.package] version` in `Cargo.toml` and `version` in `desktop/tauri.conf.json`, touching only those lines.
- Refreshes `Cargo.lock` for the workspace crates only, without updating any dependency, so `cargo … --locked` in CI passes.
- Prints the next steps.
- POSIX sh, so it runs on macOS and Linux the way `build-env.sh` does.

## Acceptance criteria
- [x] Running it updates Cargo.toml, tauri.conf.json and Cargo.lock to the new version and nothing else
- [x] It refuses malformed versions and versions not higher than the current one
- [x] cargo build --workspace --locked passes after a bump
- [ ] The version comparison is covered by tests run in CI

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-68a3 · started

### 2026-10-22 · claude-code-68a3 · finished
Added scripts/bump-version.sh, which sets the version in Cargo.toml, desktop/tauri.conf.json and Cargo.lock (`cargo update --workspace --offline`, so dependencies don't change). It refuses malformed versions, versions that aren't higher by SemVer precedence, files that already disagree, and uncommitted changes to the three files. It warns when the branch isn't version/vX.Y.Z and prints the next steps. The version rules and comparison live in scripts/version.sh, which .github/scripts/build-env.sh now sources instead of keeping its own regex. The spec documents both. I checked by hand under sh and dash: the comparison cases, every tag the old build-env test covered, the refusals, and a real 0.1.0 → 0.2.0-alpha.1 bump (only the three version lines changed and `cargo build --workspace --locked` passed). Then I reverted the bump.
Files: scripts/version.sh, scripts/bump-version.sh, .github/scripts/build-env.sh, docs/spec.md
Unmet: 4 (By the user's decision there are no CI tests for the shell scripts (the earlier build-env test job was removed on purpose). The comparison was checked by hand under sh and dash instead.)

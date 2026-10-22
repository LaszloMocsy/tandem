---
id: itjpz9e
title: CI check and PR template for PRs into main
status: finished
priority: high
parent: b85qh8f
labels: [area:release, feat/versioning]
depends_on: [tenmi47]
created: 2026-10-22
---

## Description
Enforce the version flow on pull requests into `main`.

- Its own workflow, `.github/workflows/version.yml`, triggered by `pull_request` with `branches: [main]`, so it doesn't run for PRs into version branches and can be marked as a required check by name. Its one job:
  - fails unless the head branch is `version/vX.Y.Z`, so feature branches go into a version branch first;
  - fails unless the branch name, `Cargo.toml`, `tauri.conf.json` and `Cargo.lock` all agree on the version;
  - fails unless that version is higher than the one on `main`.
- The checks are a new `cargo xtask version check --base <ref> [--branch <name>]` command with unit tests, built on the xtask version logic.
- `.github/pull_request_template.md` with a short checklist: version bumped with `cargo xtask version bump`, release notes reviewed, spec updated if behaviour changed.
- Note in the docs that the job must be made a required check in branch protection. That is a manual GitHub setting.
- The `release-versioning` branch itself is exempt, as the last feature branch merged straight into `main`; say how (for example, merge before the check is required).

## Acceptance criteria
- [x] A PR into main from a non-version branch fails the version job
- [x] A PR into main whose version isn't higher than main's, or whose files disagree, fails with a clear message
- [x] PRs into version branches don't run the job
- [x] The PR template exists and lists the release checklist

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-e6c9 · started

### 2026-10-22 · claude-code-e6c9 · finished
Added `cargo xtask version check --base REF --branch NAME` (options in any order, combinable with --tag). It fails unless the branch is version/vX.Y.Z matching the version in all three files, and unless that version is higher than Cargo.toml's on REF. Every message says what to do next. The rules live in xtask/src/check.rs with 5 unit tests (14 xtask tests in total). New .github/workflows/version.yml runs only on pull requests into main (also on `edited`, so retargeting re-runs it): it checks out the merge commit, fetches main, and runs the check with the branch name passed through env. Added .github/pull_request_template.md with a general checklist and a Release section for version PRs. CONTRIBUTING.md has a new Branches section with the local command and the branch-protection setup (required "Version" check, ideally "require up to date"). The spec has a Branches bullet and lists version.yml. Checked locally against real refs: a non-version branch, a mismatched branch, a version not higher than main, disagreeing files, and a bad ref all fail with clear messages; a bumped version/v0.2.0 branch passes. The workflow itself hasn't run on GitHub yet. Exemption for release-versioning: its PR into main will fail the Version check, since it isn't a version branch. Merge it before making Version a required check (branch protection isn't on for main today).
Files: xtask/src/check.rs, xtask/src/main.rs, .github/workflows/version.yml, .github/pull_request_template.md, CONTRIBUTING.md, docs/spec.md
Follow-ups: After merging release-versioning, make "Version" a required status check for main in branch protection.

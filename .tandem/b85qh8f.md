---
id: b85qh8f
title: "Versioned release workflow: feature → version/vX.Y.Z → main"
status: done
priority: high
labels: [area:release, feat/versioning]
created: 2026-10-22
reviewed_at: 2026-10-22
---

## Description
Move to a `feature A`, `feature B` → `version/vX.Y.Z` → `main` workflow. Feature branches merge into a version branch; only version branches merge into `main`, and merging one tags and releases it.

All child tasks land on the `release-versioning` branch, the last feature branch that goes into `main` on its own.

The version lives in `Cargo.toml` (`[workspace.package]`), `desktop/tauri.conf.json` and `Cargo.lock` (the three workspace crates). `release.yml` already refuses a tag that doesn't match the first two.

## Acceptance criteria
- [x] Every child task is done
- [x] docs/spec.md describes the branch flow, the bump script, the version check, auto-tagging, release notes and the version in the app
- [x] README or CONTRIBUTING explains how to cut a release

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · László Mócsy · started

### 2026-10-22 · László Mócsy · released

### 2026-10-22 · claude-code-c175 · started

### 2026-10-22 · claude-code-c175 · finished
All seven child tasks are done (bump tooling → cargo xtask, the Version check and PR template, auto-tagging, release notes, the version in Settings, and the About links). I checked the docs against what they shipped and filled the gaps. Spec: the Versions bullet now lists every xtask command, including previous-tag, and spells out each check option separately; Branches says every release, pre-releases included, gets a version branch named after its exact version, cut from main. CONTRIBUTING: feature branches start from and target their version branch; pre-release branch naming; a one-time maintainer setup list (required Version check, create the skip-release-notes label); and a correct way to retry a release (delete the draft, then `gh workflow run release.yml --ref vX.Y.Z`). CLAUDE.md lists the xtask version commands. The rest of the flow was documented by the child tasks: the spec's Branches, Releases, Versions and App settings sections, and CONTRIBUTING's Branches and Releasing.
Files: docs/spec.md, CONTRIBUTING.md, CLAUDE.md
Follow-ups: After merging release-versioning into main: make Version a required check, create the skip-release-notes label, and decide what to do with the local-only v0.1.0 tag. Watch the first version/vX.Y.Z merge for the Tag and Release runs and the grouped notes. Try the About links in cargo tauri dev.

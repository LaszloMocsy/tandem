---
id: wybnwb4
title: Tag automatically when a version branch merges into main
status: done
priority: medium
parent: b85qh8f
labels: [area:release, feat/versioning]
depends_on: [tenmi47]
created: 2026-10-22
reviewed_at: 2026-10-22
---

## Description
A workflow that runs when a pull request from `version/vX.Y.Z` merges into `main`. It tags the merge commit `vX.Y.Z`, which starts `release.yml`.

- Read the version from the files on the merge commit, not only the branch name, and fail if they disagree.
- Refuse if the tag already exists.
- Tags pushed with the default `GITHUB_TOKEN` don't trigger other workflows. Use a token or GitHub App that can, or call `release.yml` through `workflow_call`/`workflow_dispatch`. Document the choice and any secret to set.
- Pin third-party actions to SHAs, as `release.yml` does.

## Acceptance criteria
- [ ] Merging a version PR into main creates the vX.Y.Z tag on the merge commit
- [ ] The tag starts the release workflow and produces a draft release
- [x] Merging any other PR creates no tag
- [x] An existing tag makes the workflow fail without moving it

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-d6d6 · started

### 2026-10-22 · claude-code-d6d6 · finished
New .github/workflows/tag.yml runs when a pull request into main closes, and only for merged PRs from version/* branches. It checks out the merge commit and reads the version with `cargo xtask version check --branch <head>`, which also checks that the version files agree with each other and with the branch name. It refuses a tag that already exists on origin, pushes the tag vX.Y.Z on the merge commit, and starts release.yml with `gh workflow run --ref vX.Y.Z`. A tag pushed with GITHUB_TOKEN doesn't trigger workflows, but workflow_dispatch is the exception, so no extra token or secret is needed. Permissions are contents: write and actions: write on the job only, the rust-toolchain action is pinned, there's no cache, and the branch name goes through env. release.yml gains `workflow_dispatch` plus a first step that fails unless it runs for a tag. bump's next steps, CONTRIBUTING's Releasing steps and the spec's Releases bullet describe the flow. A hand-pushed tag still starts a release. Checked locally: the version read step, the ls-remote existence check (exit 0 for existing, 2 for missing), and the YAML parse. The workflows haven't run on GitHub yet. Found on the way: origin has no tags; v0.1.0 exists only locally.
Files: .github/workflows/tag.yml, .github/workflows/release.yml, xtask/src/main.rs, CONTRIBUTING.md, docs/spec.md
Follow-ups: Watch the first version/vX.Y.Z merge: the Tag run should push the tag and start a Release run for it. The local-only tag v0.1.0 was never pushed; decide whether to push it or delete it.
Unmet: 1 (Can only be checked on GitHub: the workflow first runs when a version/vX.Y.Z PR merges into main. The steps were checked locally.); 2 (Same: starting release.yml through workflow_dispatch on the new tag can only be checked on the first real version merge.)

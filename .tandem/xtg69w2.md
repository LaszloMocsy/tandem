---
id: xtg69w2
title: Generate release notes from merged PRs
status: finished
priority: medium
parent: b85qh8f
labels: [area:release, feat/versioning]
depends_on: [tenmi47]
created: 2026-10-22
---

## Description
Replace the fixed release body in `release.yml` with generated notes, keeping the download and unsigned-build text.

- Use GitHub's generated notes (`gh api repos/…/releases/generate-notes`) against the previous tag of the same kind. For a production release, compare with the previous production tag so pre-release notes roll up.
- Add `.github/release.yml` categories (features, fixes, other) keyed on PR labels, and say which labels to use.
- With version branches, the PRs listed should be the feature PRs merged into the version branch, not only the version PR. Check what GitHub's generator returns and adjust if needed (for example, by listing commits or PRs between the tags).

## Acceptance criteria
- [ ] A draft release body lists the changes since the previous release, grouped by category
- [x] The download and unsigned-build text is still there
- [x] Feature PRs merged through a version branch appear in the notes
- [x] User accepted what the release notes will look like

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-2ab3 · started

### 2026-10-22 · claude-code-2ab3 · finished
The draft release body is now the download and unsigned-build text followed by GitHub's generated notes (`releases/generate-notes`). They start from the tag given by a new `cargo xtask version previous-tag TAG`: the last production tag for a production release, so its notes roll up the alphas and betas, or the last tag of any kind for a pre-release; non-release tags are ignored. It has 2 unit tests (16 xtask tests in total). The create-release checkout now fetches full history so the tags are there. New .github/release.yml groups the notes by the repo's existing labels: enhancement → New features, bug → Fixes, documentation → Documentation, everything else → Other changes. A `skip-release-notes` label excludes a PR. I checked the version-branch question against this repo: generated notes list PR #5, which was merged into feat/project-management rather than main, so feature PRs merged through a version branch appear as long as the version PR is merged with a merge commit. CONTRIBUTING, the PR template and the spec say so (label feature PRs, merge version PRs without squashing, label them skip-release-notes). I ran the notes step locally in bash against the real API. The user accepted the format as shown (download text, grouped changes, Full Changelog link). The categories config only takes effect once it's on main, and the `skip-release-notes` label doesn't exist on GitHub yet.
Files: .github/workflows/release.yml, .github/release.yml, xtask/src/version.rs, xtask/src/main.rs, .github/pull_request_template.md, CONTRIBUTING.md, docs/spec.md
Follow-ups: Create the `skip-release-notes` label on GitHub. Check the grouping on the first release's draft.
Unmet: 1 (Generation and the previous-tag choice are in place and the notes step was run locally, but the grouping by category comes from .github/release.yml, which GitHub reads from the repo, so it can only be seen after this lands on main and the first release runs.)

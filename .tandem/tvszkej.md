---
id: tvszkej
title: Release fetcher with channels and per-OS files
status: todo
priority: high
parent: ybd67we
labels: [area:website]
depends_on: [itnff5i]
created: 2026-10-22
---

## Description
A build-time module in `site/` that turns GitHub releases into download data for the page.

- Fetch `GET /repos/LaszloMocsy/tandem/releases` at build time; use `GITHUB_TOKEN` when set to avoid rate limits. Drafts are ignored.
- Classify by tag, following the spec's build environments: `vX.Y.Z` production, `vX.Y.Z-beta.N` beta, `vX.Y.Z-alpha.N` alpha; ignore any other tag.
- Keep the newest release per channel. Beta and alpha are only returned when newer than the production release.
- Match assets per OS by pattern, not by version: macOS `_aarch64.dmg` (main) and `_x64.dmg` (Intel); Windows `-setup.exe`; Linux `.AppImage` (main), `.deb`, `.rpm`.
- Expose version, date, URLs and an "is there a production build" flag.
- On any fetch or parse failure, fall back to the releases page URL for every link and don't fail the build.

## Acceptance criteria
- [ ] Unit tests cover tag classification, including tags that match no channel
- [ ] Unit tests cover picking the newest per channel and hiding beta/alpha that aren't newer than production
- [ ] Unit tests cover asset matching for every OS and a release missing an asset
- [ ] A failed fetch falls back to the releases page and the build still succeeds
- [ ] No version or file name is hard-coded

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

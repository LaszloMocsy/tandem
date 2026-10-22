---
id: tvszkej
title: Release fetcher with channels and per-OS files
status: finished
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
- [x] Unit tests cover tag classification, including tags that match no channel
- [x] Unit tests cover picking the newest per channel and hiding beta/alpha that aren't newer than production
- [x] Unit tests cover asset matching for every OS and a release missing an asset
- [x] A failed fetch falls back to the releases page and the build still succeeds
- [x] No version or file name is hard-coded

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-56e5 · started

### 2026-10-22 · claude-code-56e5 · finished
Added site/src/lib/releases.ts: fetches the repo's releases at build time (GITHUB_TOKEN sent when set), skips drafts, classifies tags as production/beta/alpha per the spec and ignores the rest, keeps the newest per channel by semver, and drops beta/alpha unless newer than production. Assets are matched by suffix (_aarch64.dmg, _x64.dmg, -setup.exe, .AppImage, .deb, .rpm); a missing file links to its release page. Exposes version, tag, date, release URL, files and hasProduction; getDownloads() memoises one fetch per build. Any fetch, status or parse failure logs a warning and returns fallback data with every link on the releases page. 19 bun tests in releases.test.ts; added a `test` script (bun test), @types/bun, and "types": ["bun"] in tsconfig since TS 6 no longer loads @types by default. Checked in a real build with a temporary page: live data resolves v0.1.0's six files, and a forced failure falls back and the build completes.
Files: site/src/lib/releases.ts, site/src/lib/releases.test.ts, site/package.json, site/bun.lock, site/tsconfig.json
Follow-ups: The only release, v0.1.0, is titled "Alpha" and marked pre-release on GitHub, but its tag is plain vX.Y.Z, so by the spec's tag rules it classifies as production and the page would offer it as the stable download. Either retag it (e.g. v0.1.0-alpha.1) or decide the fetcher should also respect GitHub's prerelease flag.

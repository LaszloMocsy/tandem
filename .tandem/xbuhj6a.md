---
id: xbuhj6a
title: Link to the repo and release notes from Settings → About
status: finished
priority: low
parent: b85qh8f
labels: [area:release, area:ui, feat/versioning]
created: 2026-10-22
---

## Description
Follow-up to `3vhh33b`. Settings → App → About shows the running version but no links, because the app can't open external URLs yet: links in the webview don't open the system browser.

- Add a way to open external URLs in the default browser. The likely route is the official `tauri-plugin-opener` (Rust crate in `desktop`, `@tauri-apps/plugin-opener` in `ui`, and its permission in `desktop/capabilities/`), scoped to `https://github.com/LaszloMocsy/tandem/*` only. This is a new dependency, so confirm the choice before adding it.
- In About, add two links next to the version:
  - **Repository:** `https://github.com/LaszloMocsy/tandem`
  - **Release notes:** `https://github.com/LaszloMocsy/tandem/releases/tag/v<version>` for the running version. Dev builds link to the releases list instead, since their version may not be released yet.
- Keep `core` and `mcp` free of Tauri; this lives in `desktop` and `ui` only.

## Acceptance criteria
- [ ] Clicking Repository or Release notes opens the system browser, not the app window
- [x] Release notes links to the running version's release; dev builds link to the releases list
- [x] The opener only allows the repo's GitHub URLs
- [ ] Covered by frontend tests, and tried in cargo tauri dev
- [x] The spec's About description mentions the links

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-51e4 · started

### 2026-10-22 · claude-code-51e4 · finished
Settings → App → About now has two link buttons under the version: Release notes (shown as Releases in dev builds) and Repository. As the user chose, tauri-plugin-opener 2.7.2 is used from Rust only. A new `open_link` command takes `repository` or `releaseNotes`, and desktop/src/links.rs builds the URL from CARGO_PKG_REPOSITORY: the repo, `/releases/tag/v<version>` for alpha/beta/production, or `/releases` for dev. There's no npm package and no opener permission in capabilities, so the window can't open arbitrary URLs. A failure shows inline. Tests: 3 Rust unit tests (URLs per environment; only the two camelCase link names deserialize) and the Settings test checks the buttons and dev wording. Spec updated. fmt, clippy, cargo test, oxlint, tsc and vitest pass. The opener pulls in about 35 new crates on Linux (zbus and friends, for xdg-desktop-portal).
Files: desktop/Cargo.toml, Cargo.lock, desktop/src/links.rs, desktop/src/lib.rs, desktop/src/commands.rs, ui/src/api.ts, ui/src/Settings.tsx, ui/src/Settings.test.tsx, docs/spec.md
Follow-ups: Try both links in `cargo tauri dev` (quit the Tandem app first): each should open the system browser, with Releases for the dev build.
Unmet: 1 (Not clicked in the running app. A `cargo tauri dev` run would share the data folder and MCP port with the Tandem app already running (which serves this task's MCP), so I left that to the user.); 4 (Frontend and Rust tests are in place, but the cargo tauri dev check is left to the user for the same reason.)

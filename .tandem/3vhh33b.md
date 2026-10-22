---
id: 3vhh33b
title: Show the app version in the app
status: done
priority: medium
parent: b85qh8f
labels: [area:release, area:ui, feat/versioning]
created: 2026-10-22
reviewed_at: 2026-10-22
---

## Description
Show the running version so bug reports carry it. Today production builds show it nowhere; only ALPHA and BETA show it in the top strip.

- Read it from Tauri (`app.package_info().version` or `getVersion()` from `@tauri-apps/api/app`), so it always matches `tauri.conf.json`.
- Show it in an About section of Settings (`ui/src/Settings.tsx`): "Tandem v0.2.0", plus the environment for DEV/ALPHA/BETA, and links to the repo and the release notes for that version.
- The about text should be easy to select or copy for bug reports.

## Acceptance criteria
- [x] Settings shows the running version in every environment
- [x] The version comes from the app, not a hard-coded string
- [x] DEV, ALPHA and BETA builds show their environment next to the version
- [x] Covered by a frontend test

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-b5a1 · started

### 2026-10-22 · claude-code-b5a1 · finished
Settings → App now ends with an About section: "Version" with `Tandem v0.2.0` in a code span and a Copy button, ready to paste into a bug report. Non-production builds add the build, as in `Tandem v0.2.0-beta.1 (BETA build)` or `(DEV build)`. The text comes from a new `versionText(env, version)` in buildEnv.ts. It uses the existing APP_VERSION and BUILD_ENV, which Vite reads from tauri.conf.json and TANDEM_ENV at build time (the same source as the strip's label), instead of Tauri's async getVersion(). AppSettingsView takes optional env/version props for tests. Added tests for versionText and for the About section in every environment, including the default (running dev build). Spec updated. oxlint, tsc and vitest (91 tests) pass. Not tried in the running app; it reuses the existing settings-values and CopyButton styles. Left out the repo and release-notes links from the description: the app has no way to open external URLs yet, which would need tauri-plugin-opener (a new dependency to discuss first).
Files: ui/src/buildEnv.ts, ui/src/buildEnv.test.ts, ui/src/Settings.tsx, ui/src/Settings.test.tsx, docs/spec.md
Follow-ups: Links to the repo and to this version's release notes in About, once the app can open external URLs (tauri-plugin-opener). Worth a look in `cargo tauri dev`.

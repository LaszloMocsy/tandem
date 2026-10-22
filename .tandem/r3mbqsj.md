---
id: r3mbqsj
title: Download component with OS detection and switcher
status: done
priority: high
parent: ybd67we
labels: [area:website]
depends_on: [tvszkej]
created: 2026-10-22
reviewed_at: 2026-10-22
---

## Description
One component used in the hero and the closing call to action, fed by the release fetcher.

- OS tabs for macOS, Windows and Linux, built as radio inputs so switching works without JS. No-JS default: macOS.
- A small inline script picks the tab from `navigator.userAgentData.platform`, falling back to the user agent. Phones, tablets and unknown systems stay on macOS with the note "Tandem is a desktop app."
- Main button: "Download for {OS} · v{version}" for the latest production build. macOS: Apple silicon main, "Intel Mac?" link to x64. On Chromium, `getHighEntropyValues(["architecture"])` can detect x86 and swap them. Windows: `-setup.exe`. Linux: AppImage main, `.deb` and `.rpm` links.
- With no production build, the main button reads "No stable release yet" and is not a link.
- Under it, a small muted line: "Pre-releases: Beta v… · Alpha v…", linking to the same OS's file, shown only for channels the fetcher returns.
- Per-OS unsigned-build note in `<details>` (Gatekeeper and Open Anyway, the "is damaged" quarantine fix, SmartScreen), following the selected tab, with text taken from the README.
- The hero and closing instances stay in sync.

## Acceptance criteria
- [x] The detected OS is selected on load, and the user can switch OS with or without JS
- [x] The main button links to the latest production build for the selected OS
- [x] With no production build, the button reads "No stable release yet"
- [x] The beta/alpha line is small and muted and shows only newer pre-releases
- [x] The unsigned-build note matches the selected OS and the README
- [x] Tabs and links are keyboard and screen-reader accessible

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-5c99 · started

### 2026-10-22 · claude-code-5c99 · finished
Added site/src/components/Download.astro, fed by getDownloads() (or a `downloads` prop). OS tabs are visually hidden radio inputs in a fieldset with labels, and CSS :has() shows the matching panel, so switching works without JS (default macOS) and keyboard/screen readers get a native radio group with a visible focus ring. One deduped inline module script picks the tab via detectOs() (new src/lib/platform.ts: userAgentData.platform, falling back to the UA; phones, tablets incl. iPadOS, ChromeOS and unknown systems stay on macOS with "Tandem is a desktop app."), flags Intel Macs via getHighEntropyValues(["architecture"]) to swap the x64 and aarch64 links, and keeps all instances on the same tab. Panels: macOS Apple silicon main with "Intel Mac?" link, Windows -setup.exe, Linux AppImage with .deb/.rpm links; "No stable release yet" as a non-link when there's no production build; a small muted "Pre-releases: Beta v… · Alpha v…" line per OS only for channels the fetcher returns; a per-OS <details> note with the README's Gatekeeper/"is damaged"/SmartScreen/Linux install text. index.astro now renders a hero and a closing #download instance. 5 new bun tests for detectOs. Checked in the browser: tab sync by click and arrow keys, Intel swap, mobile note, no-production and pre-release states (temporary page with fake data, removed), dark mode.
Files: site/src/components/Download.astro, site/src/lib/platform.ts, site/src/lib/platform.test.ts, site/src/pages/index.astro
Follow-ups: Without JS the hero and closing instances switch independently; they sync only with JS. The hero task (u6h7829) should keep <Download id="hero" /> in the new hero. v0.1.0 still shows as the stable download until it's retagged or the fetcher respects GitHub's pre-release flag (see tvszkej follow-up).

---
id: r3mbqsj
title: Download component with OS detection and switcher
status: todo
priority: high
parent: ybd67we
labels: [area:website]
depends_on: [tvszkej]
created: 2026-10-22
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
- [ ] The detected OS is selected on load, and the user can switch OS with or without JS
- [ ] The main button links to the latest production build for the selected OS
- [ ] With no production build, the button reads "No stable release yet"
- [ ] The beta/alpha line is small and muted and shows only newer pre-releases
- [ ] The unsigned-build note matches the selected OS and the README
- [ ] Tabs and links are keyboard and screen-reader accessible

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

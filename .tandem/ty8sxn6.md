---
id: ty8sxn6
title: Encode the hero video and reach Lighthouse 95+
status: todo
priority: medium
parent: ybd67we
labels: [area:website, human]
depends_on: [r3mbqsj, u6h7829, zy8iwii, 9cswthm]
created: 2026-10-22
---

## Description
Turn the raw recordings into the final media and tune the page.

- Encode with the `ffmpeg` commands from `RECORDING.md`: AV1 WebM and H.264 MP4, each under about 1.5 MB, plus a poster frame at 3s. Pick the light or dark take to match the color scheme if size allows; otherwise use the light take.
- Replace the hero screenshot with the video; `preload="none"`, poster shown first, and only the poster with `prefers-reduced-motion`.
- If clips A, B or C exist, use them in How it works; otherwise keep the HTML mocks.
- Run Lighthouse on mobile and desktop, fix what keeps performance or accessibility under 95, and record the scores in the finish summary.

## Acceptance criteria
- [ ] The hero plays the encoded loop, and each file is about 1.5 MB or less
- [ ] With `prefers-reduced-motion`, only the poster shows
- [ ] Lighthouse performance and accessibility are 95+ on mobile and desktop
- [ ] Client JS is limited to OS detection, tabs and copy buttons

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

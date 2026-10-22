---
id: itnff5i
title: Scaffold the site/ Astro project
status: todo
priority: high
parent: ybd67we
labels: [area:website]
created: 2026-10-22
---

## Description
Create `site/` as a static Astro project managed with bun, the base every other landing-page task builds on.

- Static output only; `@astrojs/sitemap`.
- Design tokens in plain CSS: neutrals copied from `ui/src/index.css` (light and dark via `prefers-color-scheme`), plus the logo's human coral (`#ff6b5b` → `#ffb47a`) and agent blue-violet (`#5a5cff` → `#7fb2ff`).
- `system-ui` stack plus a system monospace stack; no web fonts.
- Logo and favicon from `ui/public/logo.svg`; an Open Graph image built from the logo.
- Base layout with nav (logo, How it works, Download, GitHub) and footer (GitHub, Releases, License). No Docs link until #10.
- Leave room for Starlight at `/docs` later (#10).
- Scripts: `dev`, `build`, `preview`, `check` (`astro check`).

## Acceptance criteria
- [ ] `bun install && bun run build` in `site/` produces a static `dist/` with no client JS
- [ ] `bun run check` passes
- [ ] Tokens cover light and dark, and the page follows the system setting
- [ ] Nav, footer, favicon and Open Graph image are in place
- [ ] `site/` build output and `node_modules` are gitignored

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

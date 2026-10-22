---
id: itnff5i
title: Scaffold the site/ Astro project
status: done
priority: high
parent: ybd67we
labels: [area:website]
created: 2026-10-22
reviewed_at: 2026-10-22
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
- [x] `bun install && bun run build` in `site/` produces a static `dist/` with no client JS
- [x] `bun run check` passes
- [x] Tokens cover light and dark, and the page follows the system setting
- [x] Nav, footer, favicon and Open Graph image are in place
- [x] `site/` build output and `node_modules` are gitignored

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-4537 · started

### 2026-10-22 · claude-code-4537 · finished
Scaffolded site/ as a static Astro 7 project managed with bun, with @astrojs/sitemap and dev/build/preview/check scripts. Tokens in src/styles/tokens.css copy the app's neutrals and add the logo's human and agent gradients, switching to dark via prefers-color-scheme; fonts are system-ui plus a system monospace stack. BaseLayout has nav (logo, How it works, Download, GitHub), footer (GitHub, Releases, License), skip link, canonical, OG and Twitter meta. scripts/brand-assets.ts (`bun run assets`) builds favicon.svg/.ico, apple-touch-icon and a 1200x630 og.png from public/logo.svg; output is committed so the OG text renders the same every time. /docs is left free for Starlight. index.astro is a placeholder hero with empty #how-it-works and #download anchors for later tasks.
Files: site/.gitignore, site/astro.config.mjs, site/bun.lock, site/package.json, site/tsconfig.json, site/scripts/brand-assets.ts, site/public/logo.svg, site/public/favicon.svg, site/public/favicon.ico, site/public/apple-touch-icon.png, site/public/og.png, site/src/consts.ts, site/src/styles/tokens.css, site/src/styles/global.css, site/src/layouts/BaseLayout.astro, site/src/components/Nav.astro, site/src/components/Footer.astro, site/src/pages/index.astro
Follow-ups: Nav links point at /#how-it-works and /#download anchors, which the section tasks should keep. The placeholder intro in index.astro gets replaced by the hero task.

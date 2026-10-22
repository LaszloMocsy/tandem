---
id: t38dkrm
title: Add site/ to the spec's repo layout
status: done
priority: low
parent: ybd67we
labels: [area:website]
created: 2026-10-22
reviewed_at: 2026-10-22
---

## Description
Update `docs/spec.md` "Repo and open-source setup": add `site/` to the repo tree, and one bullet describing the site (static Astro, hosted on Vercel at `tandem.laszlomocsy.com`, builds only when `site/**` changes, redeploys when a release is published). Mirror the layout change in `CLAUDE.md`'s Layout block.

## Acceptance criteria
- [x] The spec's repo tree lists `site/` with a short comment
- [x] A bullet in "Repo and open-source setup" describes the site, its hosting and when it deploys
- [x] `CLAUDE.md` Layout lists `site/`

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-3e99 · started

### 2026-10-22 · claude-code-3e99 · finished
Added site/ to the spec's repo tree and a "Landing page" bullet in "Repo and open-source setup": static Astro site managed with bun, on Vercel at tandem.laszlomocsy.com, site/ as the project root with builds skipped unless site/** changes, production deploys from main, previews for PRs, and a redeploy when a release is published. Mirrored site/ in CLAUDE.md's Layout block.
Files: docs/spec.md, CLAUDE.md

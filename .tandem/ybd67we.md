---
id: ybd67we
title: Landing page (#9)
status: todo
priority: medium
labels: [area:website, human]
created: 2026-10-22
---

## Description
Tracks GitHub issue https://github.com/LaszloMocsy/tandem/issues/9. The child tasks hold the breakdown. Commits for any child carry `Refs #9` alongside their `Task:` trailer; the final PR uses `Closes #9`.

A static Astro site in `site/` that explains Tandem and links to the downloads, hosted on Vercel at `tandem.laszlomocsy.com`.

Decisions:
- t3.codes is a structural reference only (narrative single page, one claim plus one product mock per section, real commands as copy). Don't copy its look, voice, tweet wall or logo row.
- Visual identity comes from the logo: coral for the human, blue-violet for the agent, app neutrals from `ui/src/index.css`, light and dark following the system, `system-ui` plus a monospace face, no web fonts.
- Voice: calm and exact. Headline: "A task board you share with your coding agents."
- Downloads: the main button is the latest production build for the detected OS. Under it, a small muted line links the latest beta and alpha, only when newer than production. With no production build, the main button reads "No stable release yet". The user can switch OS with tabs that work without JS.
- Docs link stays out of the nav until #10 ships.

## Acceptance criteria
- [ ] `bun run build` in `site/` produces a static site with no client JS beyond what the page needs
- [ ] Download links always point at the latest release, with no manual edits per release
- [ ] Lighthouse scores 95+ for performance and accessibility
- [ ] PRs that touch `site/` get a Vercel preview deployment, and pushes to `main` deploy to `tandem.laszlomocsy.com`
- [ ] Spec "Repo and open-source setup" is updated with `site/`

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

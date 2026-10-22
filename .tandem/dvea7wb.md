---
id: dvea7wb
title: Set up Vercel, the domain and the deploy hook
status: todo
priority: medium
parent: ybd67we
labels: [area:website, human]
depends_on: [ryxe6p7]
created: 2026-10-22
---

## Description
Human task: Vercel and DNS setup for `tandem.laszlomocsy.com`.

- Import the repo through Vercel's Git integration; root directory `site/`, framework Astro, install with bun.
- Turn on "skip deployments when there are no changes to the root directory".
- Confirm that pushes to `main` deploy to production and PRs get previews.
- Add `tandem.laszlomocsy.com` and point a CNAME at the target Vercel shows.
- Optionally add a read-only `GITHUB_TOKEN` env var for the release fetcher's rate limit.
- Create a deploy hook for `main` and save it as the `VERCEL_DEPLOY_HOOK_URL` repo secret.

## Acceptance criteria
- [ ] A PR touching `site/` gets a preview deployment, and one that doesn't skips the build
- [ ] A push to `main` deploys to `tandem.laszlomocsy.com` over HTTPS
- [ ] The `VERCEL_DEPLOY_HOOK_URL` secret exists, and publishing a release triggers a deploy

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

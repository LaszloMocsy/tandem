---
id: ryxe6p7
title: Redeploy the site when a release is published
status: todo
priority: medium
parent: ybd67we
labels: [area:website]
depends_on: [itnff5i]
created: 2026-10-22
---

## Description
Add `.github/workflows/site-deploy.yml`, triggered on `release: published`, that POSTs to a Vercel deploy hook stored as the `VERCEL_DEPLOY_HOOK_URL` secret. This rebuilds the site so the release fetcher picks up the new release with no manual edits. Releases start as drafts, so `published` is the right event. Fail clearly if the secret is missing. Check whether Vercel's "skip unaffected" setting also skips deploy-hook builds, and note the answer in the workflow.

## Acceptance criteria
- [ ] The workflow runs on `release: published` and calls the deploy hook
- [ ] A missing secret fails the run with a clear message
- [ ] The hook URL never appears in logs
- [ ] Whether "skip unaffected" affects deploy hooks is checked and noted

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

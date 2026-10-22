---
id: ryxe6p7
title: Redeploy the site when a release is published
status: finished
priority: medium
parent: ybd67we
labels: [area:website]
depends_on: [itnff5i]
created: 2026-10-22
---

## Description
Add `.github/workflows/site-deploy.yml`, triggered on `release: published`, that POSTs to a Vercel deploy hook stored as the `VERCEL_DEPLOY_HOOK_URL` secret. This rebuilds the site so the release fetcher picks up the new release with no manual edits. Releases start as drafts, so `published` is the right event. Fail clearly if the secret is missing. Check whether Vercel's "skip unaffected" setting also skips deploy-hook builds, and note the answer in the workflow.

## Acceptance criteria
- [x] The workflow runs on `release: published` and calls the deploy hook
- [x] A missing secret fails the run with a clear message
- [x] The hook URL never appears in logs
- [ ] Whether "skip unaffected" affects deploy hooks is checked and noted

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-77f0 · started

### 2026-10-22 · claude-code-77f0 · finished
Added .github/workflows/site-deploy.yml: on release published (drafts publish later, pre-releases included), one job with no token permissions POSTs to the VERCEL_DEPLOY_HOOK_URL secret with curl -fsS --retry 3, checks the response has a job id and prints only that id. A missing secret fails with an ::error that says how to create the hook and secret. The URL only reaches curl through env, GitHub masks it, and curl's errors don't include it. Exercised the step's script locally against a fake hook server: missing secret exits 1 with the message, success prints the job id, a 404 fails with exit 22, and the URL appeared in none of the outputs. YAML lint passes; actionlint isn't installed here. Read Vercel's docs (monorepos "Skipping unaffected projects", deploy hooks, monorepo FAQ) on whether skipping applies to deploy-hook builds and recorded the result and the fallbacks in the workflow header.
Files: .github/workflows/site-deploy.yml
Follow-ups: For dvea7wb: Vercel's docs say "skip unaffected projects" needs npm/yarn/pnpm/Bun workspaces with the project in the workspace definition. This repo has no root package.json or workspaces (ui/ and site/ are standalone), so the toggle may never skip anything, and every push to main may build the site. If so, an Ignored Build Step such as `git diff --quiet HEAD^ HEAD -- .` from site/ would do the skipping; but that would likely also skip deploy-hook builds, which brings back the question above.
Unmet: 4 (Checked but not settled: Vercel's docs describe skipping per commit and never say whether deploy-hook builds are subject to it, and community reports show hook builds being skipped by the Ignored Build Step. It can only be confirmed on a real project, so the workflow header notes the open question, how to spot it (a Skipped deployment after a release) and two fallbacks (turn skipping off, or deploy with the Vercel CLI and a token). Should be checked as part of dvea7wb.)

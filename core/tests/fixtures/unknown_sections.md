---
id: p2xmq7d
title: "Cache: invalidate on logout"
status: done
priority: medium
labels: [auth, needs review]
created: 2026-10-12
reviewed_at: 2026-10-14
---

Text before any section, kept as is.

## Description
Logging out leaves cached profile data behind.

## Notes from review   
Trailing spaces on the heading above and on this line are kept.   
<!-- an HTML comment -->

    indented code block
	tab-indented line


Two blank lines above, *odd* Markdown ~~here~~.

```sh
## not a heading, inside a fence
echo "keep me"
```

## Acceptance criteria
- [x] The cache is cleared on logout
- [ ] Unrelated: nothing else is cleared

## Relevant files

## Changelog

### 2026-10-12 · laszlo · created
Created on the board.

### 2026-10-13 · codex-91ab · finished
Clear the profile cache in the logout handler.
Files: src/cache.ts
Unmet: 2 (no test for other caches yet)

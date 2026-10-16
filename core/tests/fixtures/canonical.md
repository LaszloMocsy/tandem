---
id: k3f9x2m
title: Fix token refresh race
status: finished
priority: high
parent: a7rq4ne
labels: [auth, backend]
depends_on: [p2xmq7d]
created: 2026-10-13
---

## Description
Parallel requests that hit a 401 each trigger their own token refresh.

## Acceptance criteria
- [x] Concurrent requests during a refresh share one refresh call
- [x] A test covers two parallel 401 responses

## Relevant files
- src/auth/session.ts

## Changelog

### 2026-10-13 · claude-code · started
Picked up from todo.

### 2026-10-14 · claude-code · finished
Serialized refresh behind a shared promise and added a parallel-401 test.
Files: src/auth/session.ts, src/auth/session.test.ts
Follow-ups: none

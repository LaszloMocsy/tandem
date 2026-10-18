# Tasks

This folder holds the tasks for this repo, one Markdown file per task. They
are managed by [Tandem](https://github.com/LaszloMocsy/tandem), a kanban board
shared by people and AI coding agents. Commit the files together with the code
that works on them.

Agents: change tasks only through the `tandem` MCP tools. Never edit the
frontmatter or the Changelog section by hand.

## Files

- `config.toml` records the version of this format, and optionally the
  board's display name: `name = "My board"`.
- Each task is `<id>.md`, where the id is 7 characters from
  `abcdefghijkmnpqrstuvwxyz23456789`, such as `k3f9x2m.md`.
- Every other file in this folder, including this one, is ignored.

## Task format

```markdown
---
id: k3f9x2m
title: Fix token refresh race
status: in_progress
priority: high
parent: a7rq4ne
labels: [auth, backend]
depends_on: [p2xmq7d]
created: 2026-10-13
---

## Description
Parallel requests that hit a 401 each trigger their own token refresh.

## Acceptance criteria
- [ ] Concurrent requests during a refresh share one refresh call

## Relevant files
- src/auth/session.ts

## Changelog

### 2026-10-13 · claude-code · started
Picked up from todo.
```

Frontmatter fields, in this order:

| Field | Value | Notes |
| --- | --- | --- |
| `id` | task id | Never changes; also the file name |
| `title` | text | Required |
| `status` | `backlog`, `todo`, `in_progress`, `finished` or `done` | Required |
| `priority` | `high`, `medium` or `low` | Optional |
| `parent` | task id | Optional; must exist and must not form a loop |
| `labels` | list of text | Optional |
| `depends_on` | list of task ids | Optional; must not form a loop |
| `created` | date | Required |
| `reviewed_at` | date | Set when a person reviews the task into `done` |

Optional fields are left out when empty.

The body is free Markdown. The usual sections are Description, Acceptance
criteria, Relevant files and Changelog; other sections are kept as they are.
The Changelog is append-only, oldest entry first. Each entry is a
`### <date> · <who> · <event>` heading, then a summary, then optional
`Files:`, `Follow-ups:` and `Unmet:` lines.

---
id: zy8iwii
title: Features, agents and open-source sections
status: finished
priority: medium
parent: ybd67we
labels: [area:website]
depends_on: [itnff5i]
created: 2026-10-22
---

## Description
Build the rest of the page body.

- Features, about six short cards: Git is the history (Commit review commits one file); claims are leases, so a crashed agent's task can be taken over; agents propose, you approve; local-only (MCP on localhost, no account, no cloud); SQLite is a cache rebuilt from the files; keyboard shortcuts.
- Works with your agent: Claude Code, `.mcp.json`, Codex, Cursor, or any Streamable HTTP MCP client. Plain text, no logo row implying official integrations.
- Open source and closing call to action: "Tandem is built with Tandem", linking to the repo's `.tandem/`; MIT; GitHub; slot for the download component.

Every claim must match `docs/spec.md` and the README.

## Acceptance criteria
- [x] Each feature claim matches the spec or README
- [x] No agent vendor logos are used
- [x] The closing section links to the repo's `.tandem/` folder and the license
- [x] Sections work in light and dark and from 360px wide

## Changelog

### 2026-10-22 · claude-code · created

### 2026-10-22 · László Mócsy · approved

### 2026-10-22 · claude-code-bc66 · started

### 2026-10-22 · claude-code-bc66 · finished
Added the rest of the page body. Features.astro: six cards (Git is the history; Claims are leases; Agents propose, you approve; Local only; The files are the truth; Keyboard first), each with a coral or blue-violet edge for who it serves. Claims sourced from the spec (principles 1-3, the 15-minute lease extended by calls carrying the agent id, start_task only from todo) and the README (Commit review commits one file, localhost-only MCP with one endpoint per project while the app runs, SQLite in the app data folder rebuilt from files, the N / Enter R A shortcuts and drag). Agents.astro: "Works with your agent" in plain text with no logos: the claude mcp add command, the .mcp.json snippet, Codex/Cursor/any Streamable HTTP client using the same URL in its own config, and the AGENTS.md/CLAUDE.md snippet, noting the Setup panel has copy buttons. OpenSource.astro replaces the old closing section (keeps id="download" for the nav): logo, "Tandem is built with Tandem" linking to the repo's .tandem/ tree, the MIT license link, the closing Download instance and View on GitHub. Added TASKS_URL to consts. Checked in light and dark at desktop and 360px: no horizontal overflow, dark tokens apply.
Files: site/src/components/Features.astro, site/src/components/Agents.astro, site/src/components/OpenSource.astro, site/src/consts.ts, site/src/pages/index.astro
Follow-ups: "There is no account to create" isn't stated outright in the spec or README; it follows from the app having no sign-in anywhere. Drop it if you'd rather keep strictly to written claims. Codex and Cursor are named only as clients that can use the URL, per the spec's note that they use their own config formats.

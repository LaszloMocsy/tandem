// What the Setup panel offers to copy, for connecting agents to a
// project's MCP endpoint. The formats follow "Agent workflow" in the spec.

/** The Claude Code command that adds the project's server. */
export function claudeCommand(url: string): string {
  return `claude mcp add --transport http tandem ${url}`
}

/** A project-level `.mcp.json` for the repo root. */
export function mcpJson(url: string): string {
  return `{
  "mcpServers": {
    "tandem": { "type": "http", "url": "${url}" }
  }
}
`
}

/** The spec's snippet for `AGENTS.md` or `CLAUDE.md`, word for word. */
export const AGENTS_SNIPPET = `## Tasks

This repo tracks work in \`.tandem/\` through the \`tandem\` MCP server.

- Before starting work, call \`start_task\` on the task (use \`next_task\` if none was given), and pass the agent id it returns as \`agent\` to every later tandem call for that task.
- Never edit task frontmatter or the Changelog section by hand. Use the tandem tools.
- Include the task file in your first commit (status in_progress) and your final commit (status finished).
- Add a \`Task: <id>\` trailer to every commit for that task.
- Use \`log_progress\` for notes and \`set_blocked\` when you need a decision from me.
- Split large work into child tasks with \`create_task\` and \`parent\`. New tasks wait in backlog until I approve them; only start tasks in todo.
- Call \`finish_task\` with a short summary before the final commit. Tick each acceptance criterion you met and explain each one you didn't. Never mark tasks reviewed.
- If the tandem server is unreachable, ask me to open the app instead of editing files.
`

/**
 * Copies `text` to the clipboard. Falls back to a hidden textarea where the
 * Clipboard API is unavailable.
 */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text)
    return
  } catch {
    // Fall through to the older way.
  }
  const area = document.createElement('textarea')
  area.value = text
  area.style.position = 'fixed'
  area.style.opacity = '0'
  document.body.append(area)
  area.select()
  const copied = document.execCommand('copy')
  area.remove()
  if (!copied) throw new Error('could not copy to the clipboard')
}

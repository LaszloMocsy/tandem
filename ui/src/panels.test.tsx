import { renderToStaticMarkup } from 'react-dom/server'
import { expect, test } from 'vitest'
import type { AgentInfo, ProjectInfo } from './api.ts'
import { AgentList, SetupPanel } from './panels.tsx'

const NOW = Date.parse('2026-10-15T10:00:00Z')

const agents: AgentInfo[] = [
  {
    agentId: 'claude-code-7f2a',
    clientName: 'claude-code',
    lastSeenAt: '2026-10-15T09:57:00.000Z',
    tasks: [{ id: 'k3f9x2m', title: 'Fix token refresh race', live: true }],
  },
  {
    agentId: 'claude-code-91bc',
    clientName: 'claude-code',
    lastSeenAt: '2026-10-15T07:00:00.000Z',
    tasks: [{ id: 'p2xmq7d', title: '', live: false }],
  },
  { agentId: 'codex', clientName: null, lastSeenAt: '2026-10-15T09:59:40.000Z', tasks: [] },
]

const render = (list: AgentInfo[]) =>
  renderToStaticMarkup(<AgentList agents={list} now={NOW} onOpenTask={() => {}} />)

test('lists agents with when they were last seen and what they hold', () => {
  const html = render(agents)
  expect(html).toMatch(
    /aria-label="Active"><\/span><code>claude-code-7f2a<\/code><span class="muted">seen 3m ago<\/span>/,
  )
  expect(html).toContain('Fix token refresh race</button><code class="muted">k3f9x2m</code></li>')
  // Idle after a lease's length, and a stale lease is marked.
  expect(html).toMatch(/aria-label="Idle"><\/span><code>claude-code-91bc<\/code><span class="muted">seen 3h ago/)
  expect(html).toContain('p2xmq7d</button><code class="muted">p2xmq7d</code><span class="badge">lease stale</span>')
  expect(html).toContain('<code>codex</code><span class="muted">seen just now</span>')
  expect(html).toContain('Holds no task.')
})

test('says when there are no agents yet', () => {
  expect(render([])).toContain('No agents yet. Connect one from Setup.')
})

const project: ProjectInfo = {
  id: 'tandem',
  name: 'tandem',
  repoPath: '/repos/tandem',
  lastOpenedAt: null,
  open: true,
  error: null,
  readOnly: null,
  mcpUrl: 'http://127.0.0.1:4317/p/tandem/mcp',
}

test('Setup offers the URL, the Claude Code command, .mcp.json and the snippet', () => {
  const html = renderToStaticMarkup(<SetupPanel project={project} onClose={() => {}} />)
  const titles = [...html.matchAll(/<h3>([^<]+)<\/h3><button[^>]*>Copy<\/button>/g)].map((m) => m[1])
  expect(titles).toEqual(['MCP URL', 'Claude Code', '.mcp.json', 'AGENTS.md snippet'])
  expect(html).toContain('claude mcp add --transport http tandem http://127.0.0.1:4317/p/tandem/mcp')
  expect(html).toContain('&quot;url&quot;: &quot;http://127.0.0.1:4317/p/tandem/mcp&quot;')
  expect(html).toContain('This repo tracks work in `.tandem/` through the `tandem` MCP server.')
})

test('Setup explains a server that is not running, and still offers the snippet', () => {
  const html = renderToStaticMarkup(
    <SetupPanel project={{ ...project, mcpUrl: null }} onClose={() => {}} />,
  )
  expect(html).toContain('The MCP server isn&#x27;t running')
  expect(html).not.toContain('claude mcp add')
  expect(html).toContain('AGENTS.md snippet')
})

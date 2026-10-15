import { renderToStaticMarkup } from 'react-dom/server'
import { expect, test } from 'vitest'
import type { TaskSummary } from './api.ts'
import { Card } from './Card.tsx'
import type { CardData } from './columns.ts'

const NOW = Date.parse('2026-10-15T10:00:00Z')

function card(patch: Partial<TaskSummary> = {}, extra: Partial<CardData> = {}): CardData {
  return {
    task: {
      id: 'k3f9x2m',
      title: 'Fix token refresh race',
      status: 'in_progress',
      priority: 'high',
      parent: null,
      labels: [],
      dependsOn: [],
      created: '2026-10-13',
      lease: null,
      blocked: null,
      problem: null,
      unmet: [],
      ...patch,
    },
    parent: null,
    children: null,
    waitingOn: [],
    ...extra,
  }
}

const render = (data: CardData) => renderToStaticMarkup(<Card card={data} now={NOW} />)

test('shows the id, title, priority and labels', () => {
  const html = render(card({ labels: ['auth', 'backend'] }))
  expect(html).toContain('k3f9x2m')
  expect(html).toContain('Fix token refresh race')
  expect(html).toContain('aria-label="High priority"')
  expect(html).toContain('<li class="label">auth</li><li class="label">backend</li>')
  expect(html).not.toContain('class="badges"')
})

test('shows the parent breadcrumb and child progress', () => {
  const html = render(
    card(
      {},
      { parent: { id: 'a7rq4ne', title: 'Auth epic' }, children: { done: 3, total: 5 } },
    ),
  )
  expect(html).toContain('title="Part of Auth epic (a7rq4ne)">Auth epic</div>')
  expect(html).toContain('>3/5 done</span>')
})

test('shows a lock while a dependency is not done', () => {
  const waitingOn = [
    { id: 'p2xmq7d', title: 'Add session store' },
    { id: 'zzzzzzz', title: null },
  ]
  expect(render(card({}, { waitingOn }))).toContain(
    'title="Waiting on Add session store (p2xmq7d), zzzzzzz"',
  )
  expect(render(card({ status: 'done' }, { waitingOn }))).not.toContain('Waiting on')
})

test('shows a live or stale lease', () => {
  const lease = {
    agentId: 'claude-code',
    claimedAt: '2026-10-15T09:30:00.000Z',
    expiresAt: '2026-10-15T09:45:00.000Z',
    live: true,
  }
  expect(render(card({ lease: { ...lease, expiresAt: '2026-10-15T10:10:00.000Z' } }))).toContain(
    'class="badge lease live"',
  )
  const stale = render(card({ lease }))
  expect(stale).toContain('class="badge lease stale"')
  expect(stale).toContain('claude-code · stale 15m</span>')
})

test('shows blocked and invalid badges with the reason on hover', () => {
  const html = render(
    card({
      blocked: 'Need a staging account',
      problem: { kind: 'graph', message: 'parent loop: a → b → a' },
    }),
  )
  expect(html).toContain('title="Need a staging account">Blocked</span>')
  expect(html).toContain('title="parent loop: a → b → a">Invalid</span>')
})

test('names an unreadable file by its id', () => {
  const html = render(card({ title: '', problem: { kind: 'invalid_file', message: 'bad YAML' } }))
  expect(html).toContain('aria-label="k3f9x2m"')
  expect(html).toContain('Unreadable file')
})

test('shows unmet criteria on Review cards only', () => {
  const unmet = [
    { criterion: 2, text: 'Covers parallel 401s', reason: 'needs a staging account' },
    { criterion: 4, text: 'Docs', reason: 'out of scope' },
  ]
  const html = render(card({ status: 'finished', unmet }))
  expect(html).toContain(
    'title="2. Covers parallel 401s: needs a staging account\n4. Docs: out of scope">2 criteria unmet</span>',
  )
  expect(render(card({ status: 'in_progress', unmet }))).not.toContain('unmet')
})

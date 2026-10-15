import { expect, test } from 'vitest'
import type { TaskSummary } from './api.ts'
import { buildColumns, elapsed, leaseState, unmetLabel } from './columns.ts'

function task(id: string, patch: Partial<TaskSummary> = {}): TaskSummary {
  return {
    id,
    title: id,
    status: 'todo',
    priority: 'medium',
    parent: null,
    labels: [],
    dependsOn: [],
    created: '2026-10-15',
    lease: null,
    blocked: null,
    problem: null,
    unmet: [],
    ...patch,
  }
}

const ids = (cards: { task: TaskSummary }[]) => cards.map((card) => card.task.id)

test('puts each task in its status column', () => {
  const columns = buildColumns([
    task('a', { status: 'backlog' }),
    task('b', { status: 'todo' }),
    task('c', { status: 'in_progress' }),
    task('d', { status: 'finished' }),
    task('e', { status: 'done' }),
  ])
  expect(Object.fromEntries(Object.entries(columns).map(([status, cards]) => [status, ids(cards)])))
    .toEqual({ backlog: ['a'], todo: ['b'], in_progress: ['c'], finished: ['d'], done: ['e'] })
})

test('sorts by priority, then oldest created, then id; invalid files last', () => {
  const columns = buildColumns([
    task('low', { priority: 'low', created: '2026-01-01' }),
    task('newer', { priority: 'high', created: '2026-10-15' }),
    task('broken', { priority: 'high', created: null }),
    task('older', { priority: 'high', created: '2026-10-01' }),
    task('same-b', { created: '2026-10-02' }),
    task('same-a', { created: '2026-10-02' }),
  ])
  expect(ids(columns.todo)).toEqual(['older', 'newer', 'broken', 'same-a', 'same-b', 'low'])
})

test('links parents and counts done children', () => {
  const columns = buildColumns([
    task('parent', { title: 'Epic' }),
    task('c1', { parent: 'parent', status: 'done' }),
    task('c2', { parent: 'parent', status: 'done' }),
    task('c3', { parent: 'parent' }),
    task('orphan', { parent: 'missing' }),
  ])
  const card = (id: string) => columns.todo.find((c) => c.task.id === id)!
  expect(card('parent').children).toEqual({ done: 2, total: 3 })
  expect(card('parent').parent).toBeNull()
  expect(card('c3').parent).toEqual({ id: 'parent', title: 'Epic' })
  expect(card('c3').children).toBeNull()
  expect(card('orphan').parent).toEqual({ id: 'missing', title: null })
})

test('lists the dependencies that are not done, including missing ones', () => {
  const columns = buildColumns([
    task('done', { status: 'done' }),
    task('open', { title: 'Open dep' }),
    task('waiting', { dependsOn: ['done', 'open', 'gone'] }),
    task('ready', { dependsOn: ['done'] }),
  ])
  const card = (id: string) => columns.todo.find((c) => c.task.id === id)!
  expect(card('waiting').waitingOn).toEqual([
    { id: 'open', title: 'Open dep' },
    { id: 'gone', title: null },
  ])
  expect(card('ready').waitingOn).toEqual([])
})

test('a lease turns stale once it expires, even if reported live', () => {
  const lease = {
    agentId: 'claude-code',
    claimedAt: '2026-10-15T09:00:00.000Z',
    expiresAt: '2026-10-15T09:15:00.000Z',
    live: true,
  }
  const at = (time: string) => Date.parse(`2026-10-15T${time}Z`)
  expect(leaseState(lease, at('09:14:59'))).toEqual({ agentId: 'claude-code', live: true })
  expect(leaseState(lease, at('09:35:30'))).toEqual({
    agentId: 'claude-code',
    live: false,
    staleFor: '20m',
  })
  expect(leaseState({ ...lease, live: false }, at('09:15:10'))).toMatchObject({
    live: false,
    staleFor: '<1m',
  })
})

test('formats durations and unmet counts', () => {
  expect([0, 59_999, 60_000, 3_599_999, 3_600_000, 86_400_000 * 3].map(elapsed)).toEqual([
    '<1m',
    '<1m',
    '1m',
    '59m',
    '1h',
    '3d',
  ])
  expect(unmetLabel(1)).toBe('1 criterion unmet')
  expect(unmetLabel(2)).toBe('2 criteria unmet')
})

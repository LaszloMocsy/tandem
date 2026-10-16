import { expect, test } from 'vitest'
import type { Status } from './api.ts'
import { moveFor } from './moves.ts'
import { shortcutFor } from './shortcuts.ts'

const STATUSES: Status[] = ['backlog', 'todo', 'in_progress', 'finished', 'done']

test('drags apply exactly the transitions in the State model', () => {
  const moves = STATUSES.flatMap((from) =>
    STATUSES.flatMap((to) => {
      const move = moveFor(from, to)
      return move ? [`${from} > ${to}: ${move}`] : []
    }),
  )
  expect(moves).toEqual([
    'backlog > todo: approve',
    'todo > in_progress: start',
    'in_progress > todo: release',
    'in_progress > finished: finish',
    'finished > in_progress: reopen',
    'finished > done: markReviewed',
  ])
})

test('shortcuts ignore modifiers and keys typed into fields', () => {
  const key = (key: string, extra: Partial<Parameters<typeof shortcutFor>[0]> = {}) =>
    shortcutFor({ key, metaKey: false, ctrlKey: false, altKey: false, target: null, ...extra })
  expect(['n', 'N', '/', 'r', 'a', 'x', 'Enter'].map((k) => key(k))).toEqual([
    'newTask',
    'newTask',
    'search',
    'markReviewed',
    'approve',
    null,
    null,
  ])
  expect(key('n', { metaKey: true })).toBeNull()
  expect(key('a', { ctrlKey: true })).toBeNull()
})

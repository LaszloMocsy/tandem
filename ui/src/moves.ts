// Which transition a drag between two columns applies, following the
// transitions table in the spec's State model. A pair the table doesn't
// list is not a move.

import type { Status } from './api.ts'

export type Move = 'approve' | 'start' | 'finish' | 'release' | 'markReviewed' | 'reopen'

const MOVES: Partial<Record<`${Status}>${Status}`, Move>> = {
  'backlog>todo': 'approve',
  'todo>in_progress': 'start',
  'in_progress>finished': 'finish',
  'in_progress>todo': 'release',
  'finished>done': 'markReviewed',
  'finished>in_progress': 'reopen',
}

/** The transition a drag from `from` to `to` applies, or `null`. */
export function moveFor(from: Status, to: Status): Move | null {
  return MOVES[`${from}>${to}`] ?? null
}

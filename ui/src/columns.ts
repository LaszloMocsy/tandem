// What the board shows, worked out from the task list alone: the five
// columns in card order, and per card the parent, child progress, unmet
// dependencies and lease state. Kept free of React so it can be tested
// directly.

import type { Lease, Priority, Status, TaskSummary } from './api.ts'

/** The board's columns, left to right, one per status. */
export const COLUMNS: readonly { status: Status; title: string }[] = [
  { status: 'backlog', title: 'Backlog' },
  { status: 'todo', title: 'Todo' },
  { status: 'in_progress', title: 'In progress' },
  { status: 'finished', title: 'Review' },
  { status: 'done', title: 'Done' },
]

const PRIORITY_RANK: Record<Priority, number> = { high: 0, medium: 1, low: 2 }

/** A task as it relates to the others. */
export interface TaskRef {
  id: string
  /** `null` if the task is not in the list. */
  title: string | null
}

export interface CardData {
  task: TaskSummary
  parent: TaskRef | null
  /** On parent cards only. */
  children: { done: number; total: number } | null
  /** Dependencies that are not done, including missing ones. */
  waitingOn: TaskRef[]
}

export type Columns = Record<Status, CardData[]>

/**
 * Sorts by priority, then by created date, oldest first. Invalid files,
 * which have no created date, go after the rest of their priority. The id
 * breaks ties, so the order never jumps between refetches.
 */
export function compareTasks(a: TaskSummary, b: TaskSummary): number {
  return (
    PRIORITY_RANK[a.priority] - PRIORITY_RANK[b.priority] ||
    compareNullable(a.created, b.created) ||
    compareNullable(a.id, b.id)
  )
}

function compareNullable(a: string | null, b: string | null): number {
  if (a === b) return 0
  if (a === null) return 1
  if (b === null) return -1
  return a < b ? -1 : 1
}

/** Puts every task in its column, in card order. */
export function buildColumns(tasks: readonly TaskSummary[]): Columns {
  const byId = new Map(tasks.map((task) => [task.id, task]))
  const children = new Map<string, { done: number; total: number }>()
  for (const task of tasks) {
    if (task.parent === null) continue
    const progress = children.get(task.parent) ?? { done: 0, total: 0 }
    progress.total++
    if (task.status === 'done') progress.done++
    children.set(task.parent, progress)
  }
  const ref = (id: string): TaskRef => ({ id, title: byId.get(id)?.title ?? null })

  const columns: Columns = { backlog: [], todo: [], in_progress: [], finished: [], done: [] }
  for (const task of [...tasks].sort(compareTasks)) {
    columns[task.status].push({
      task,
      parent: task.parent === null ? null : ref(task.parent),
      children: children.get(task.id) ?? null,
      waitingOn: task.dependsOn.filter((id) => byId.get(id)?.status !== 'done').map(ref),
    })
  }
  return columns
}

export type LeaseState =
  | { agentId: string; live: true }
  /** `staleFor` is how long ago it expired, such as `20m`. */
  | { agentId: string; live: false; staleFor: string }

/**
 * Whether a lease is live at `now` (milliseconds since the epoch). A lease
 * the app reported live still turns stale here once it expires, since no
 * event announces that.
 */
export function leaseState(lease: Lease, now: number): LeaseState {
  const expires = Date.parse(lease.expiresAt)
  if (lease.live && !(expires <= now)) return { agentId: lease.agentId, live: true }
  return { agentId: lease.agentId, live: false, staleFor: elapsed(now - expires) }
}

/** A short duration: `<1m`, `20m`, `3h` or `2d`. */
export function elapsed(ms: number): string {
  const minutes = Math.floor(ms / 60_000)
  if (!(minutes >= 1)) return '<1m'
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  return `${Math.floor(hours / 24)}d`
}

/** `1 criterion unmet` or `2 criteria unmet`. */
export function unmetLabel(count: number): string {
  return `${count} ${count === 1 ? 'criterion' : 'criteria'} unmet`
}

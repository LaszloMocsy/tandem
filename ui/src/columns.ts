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
 * Sorts by priority, then by created date, oldest first. The id breaks
 * ties, so the order never jumps between refetches.
 */
export function compareTasks(a: TaskSummary, b: TaskSummary): number {
  return (
    PRIORITY_RANK[a.priority] - PRIORITY_RANK[b.priority] ||
    compare(a.created, b.created) ||
    compare(a.id, b.id)
  )
}

function compare(a: string, b: string): number {
  if (a === b) return 0
  return a < b ? -1 : 1
}

/** What the board shows; `null` fields don't filter. */
export interface Filters {
  label: string | null
  /** Shows the children of this task. */
  parent: string | null
  /** The ids the text search matched, or `null` without a search. */
  matches: ReadonlySet<string> | null
}

export const NO_FILTERS: Filters = { label: null, parent: null, matches: null }

export function isFiltered(filters: Filters): boolean {
  return filters.label !== null || filters.parent !== null || filters.matches !== null
}

export function isVisible(task: TaskSummary, filters: Filters): boolean {
  return (
    (filters.label === null || task.labels.includes(filters.label)) &&
    (filters.parent === null || task.parent === filters.parent) &&
    (filters.matches === null || filters.matches.has(task.id))
  )
}

/** Every label in use, sorted. */
export function labelsIn(tasks: readonly TaskSummary[]): string[] {
  return [...new Set(tasks.flatMap((task) => task.labels))].sort((a, b) => a.localeCompare(b))
}

/** The tasks that have children, by title. */
export function parentsIn(tasks: readonly TaskSummary[]): TaskSummary[] {
  const parents = new Set(tasks.map((task) => task.parent))
  return tasks
    .filter((task) => parents.has(task.id))
    .sort((a, b) => a.title.localeCompare(b.title) || a.id.localeCompare(b.id))
}

/**
 * Puts every task that passes `filters` in its column, in card order.
 * Parents, progress and dependencies still come from every task.
 */
export function buildColumns(
  tasks: readonly TaskSummary[],
  filters: Filters = NO_FILTERS,
): Columns {
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
    if (!isVisible(task, filters)) continue
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

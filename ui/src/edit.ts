// Helpers for the detail pane's editors, kept free of React so they can be
// tested directly.

import type { TaskSummary } from './api.ts'

/** One item per non-blank line, trimmed. */
export function parseLines(text: string): string[] {
  return text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line !== '')
}

export function sameList(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((item, i) => item === b[i])
}

/** The tasks a picker offers, by title, leaving out the ids in `exclude`. */
export function taskOptions(
  tasks: readonly TaskSummary[],
  exclude: readonly string[],
): TaskSummary[] {
  return tasks
    .filter((task) => !exclude.includes(task.id))
    .sort((a, b) => a.title.localeCompare(b.title) || a.id.localeCompare(b.id))
}

/** `Title (id)`, or the id alone for a task that is not in the list. */
export function taskName(tasks: readonly TaskSummary[], id: string): string {
  const title = tasks.find((task) => task.id === id)?.title
  return title ? `${title} (${id})` : id
}

/** A short local date and time for a working-log entry. */
export function formatTime(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return iso
  return date.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

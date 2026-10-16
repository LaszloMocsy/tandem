import { useEffect, useState } from 'react'
import type { Api, TaskSummary } from './api.ts'

/** How long typing pauses before a search runs. */
const DEBOUNCE_MS = 150

/**
 * The ids of the tasks that match `query` by id, title, labels or body, as
 * `list_tasks` searches them, or `null` without a query. Searches again
 * when `tasks` changes; until the first answer it keeps the last one.
 */
export function useSearch(
  listTasks: Api['listTasks'],
  project: string | null,
  query: string,
  tasks: readonly TaskSummary[],
): ReadonlySet<string> | null {
  const [matches, setMatches] = useState<ReadonlySet<string> | null>(null)
  const text = query.trim()

  useEffect(() => {
    if (project === null || text === '') return
    let current = true
    const timer = setTimeout(() => {
      listTasks(project, { query: text }).then(
        (found) => {
          if (current) setMatches(new Set(found.map((task) => task.id)))
        },
        // A failed search leaves the last results in place.
        () => {},
      )
    }, DEBOUNCE_MS)
    return () => {
      current = false
      clearTimeout(timer)
    }
  }, [listTasks, project, text, tasks])

  return text === '' ? null : matches
}

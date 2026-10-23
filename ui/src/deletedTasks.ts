// Deleted tasks in Settings → Project: listing them, Restore, and Clean up
// deleted tasks behind its confirmation. The logic is a plain object over
// the api, so it can be tested without the desktop app;
// `useDeletedTasks` wires it to React.

import { useEffect, useState, useSyncExternalStore } from 'react'
import type { Api, DeletedTask } from './api.ts'
import { errorMessage } from './edit.ts'

export type DeletedTasksApi = Pick<Api, 'listDeletedTasks' | 'restoreTask' | 'purgeDeletedTasks'>

export interface DeletedTasksState {
  /** Newest first; `null` until the first load. */
  tasks: DeletedTask[] | null
  loadError: string | null
  /** Whether the Clean up confirmation is open. */
  confirming: boolean
  busy: boolean
  /** The latest refused restore. */
  error: string | null
  /** The latest refused clean up, shown in its dialog. */
  cleanUpError: string | null
}

const initialState: DeletedTasksState = {
  tasks: null,
  loadError: null,
  confirming: false,
  busy: false,
  error: null,
  cleanUpError: null,
}

export function createDeletedTasks(api: DeletedTasksApi, project: string) {
  let state = initialState
  const listeners = new Set<() => void>()
  // Drops the results of a load that a newer one overtook.
  let loads = 0

  function set(patch: Partial<DeletedTasksState>) {
    state = { ...state, ...patch }
    for (const listener of listeners) listener()
  }

  async function load() {
    const load = ++loads
    try {
      const tasks = await api.listDeletedTasks(project)
      if (load === loads) set({ tasks, loadError: null })
    } catch (error) {
      if (load === loads) set({ loadError: errorMessage(error) })
    }
  }

  return {
    getState: () => state,

    subscribe(listener: () => void) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },

    load,

    /** Brings a task back. Refused while its parent or a dependency is deleted too. */
    async restore(id: string) {
      set({ busy: true, error: null })
      try {
        await api.restoreTask(project, id)
      } catch (error) {
        set({ error: errorMessage(error) })
      } finally {
        set({ busy: false })
        await load()
      }
    },

    /** Opens the confirmation; nothing is cleaned up yet. */
    askCleanUp() {
      set({ confirming: true, cleanUpError: null, error: null })
    },

    cancelCleanUp() {
      set({ confirming: false })
    },

    /** Removes every deleted task for good, from the confirmation. */
    async confirmCleanUp() {
      if (!state.confirming) return
      set({ busy: true, cleanUpError: null })
      try {
        await api.purgeDeletedTasks(project)
        set({ confirming: false })
      } catch (error) {
        set({ cleanUpError: errorMessage(error) })
      } finally {
        set({ busy: false })
        await load()
      }
    },
  }
}

export type DeletedTasks = ReturnType<typeof createDeletedTasks>

/**
 * The deleted tasks of `project` and their actions. `tasks` is the board's
 * task list: a change, such as a task deleted from the board or by an
 * agent, loads the list again.
 */
export function useDeletedTasks(api: DeletedTasksApi, project: string, tasks: unknown) {
  const [deleted] = useState(() => createDeletedTasks(api, project))
  const state = useSyncExternalStore(deleted.subscribe, deleted.getState, deleted.getState)
  useEffect(() => {
    void deleted.load()
  }, [deleted, tasks])
  return { ...state, actions: deleted }
}

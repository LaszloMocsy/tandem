// The board's state: the project list, the project on screen and its
// tasks. Tasks are refetched whenever `tasks-changed` names the project on
// screen, so changes from the board and agents show up the same way.
// Commands themselves go through `api`; the store never patches tasks
// locally.

import { useSyncExternalStore } from 'react'
import {
  api as tauriApi,
  CommandError,
  onTasksChanged as tauriOnTasksChanged,
  FEATURES_OFF,
  type Api,
  type Feature,
  type Features,
  type Opened,
  type ProjectInfo,
  type TaskSummary,
  type TasksChanged,
} from './api.ts'

export interface BoardState {
  /** Every project, the most recently opened first. */
  projects: ProjectInfo[]
  /** The id of the project on screen. */
  project: string | null
  /** The project's tasks, sorted by id. */
  tasks: TaskSummary[]
  /** Whether the project's tasks are loading for the first time. */
  loading: boolean
  /** The latest failed fetch, cleared by the next one that succeeds. */
  error: CommandError | null
  /** Which features are on; every one off until they load. */
  features: Features
}

export interface BoardStore {
  getState(): BoardState
  /** Calls `listener` after every state change. Returns an unsubscribe. */
  subscribe(listener: () => void): () => void
  /**
   * Starts listening for `tasks-changed`, loads the project list and shows
   * the most recently opened project that is open.
   */
  start(): Promise<void>
  /** Stops listening for events. */
  stop(): void
  /** Reloads the project list. */
  refreshProjects(): Promise<void>
  /**
   * Opens a folder and shows it. If it is not in a git repo and
   * `allowOutsideGit` is not set, nothing changes and the result says so.
   */
  openProject(path: string, allowOutsideGit?: boolean): Promise<Opened>
  /** Removes a project from the list; shows another if it was on screen. */
  removeProject(id: string): Promise<void>
  /** Refetches the tasks of the project on screen. */
  refetch(): Promise<void>
  /** Turns a feature on or off. A refusal leaves the features as they were. */
  setFeature(feature: Feature, on: boolean): Promise<void>
}

export interface BoardDeps {
  api: Pick<
    Api,
    'listProjects' | 'openProject' | 'removeProject' | 'listTasks' | 'getFeatures' | 'setFeature'
  >
  onTasksChanged: (handler: (event: TasksChanged) => void) => Promise<() => void>
}

const initialState: BoardState = {
  projects: [],
  project: null,
  tasks: [],
  loading: false,
  error: null,
  features: FEATURES_OFF,
}

export function createBoardStore(
  deps: BoardDeps = { api: tauriApi, onTasksChanged: tauriOnTasksChanged },
): BoardStore {
  const { api } = deps
  let state = initialState
  const listeners = new Set<() => void>()
  let unlisten: Promise<() => void> | null = null

  function set(patch: Partial<BoardState>) {
    state = { ...state, ...patch }
    for (const listener of listeners) listener()
  }

  function fail(error: unknown) {
    set({ error: asCommandError(error), loading: false })
  }

  // One fetch at a time. Events that arrive during a fetch ask for exactly
  // one more, so a burst of changes costs at most two fetches.
  let inFlight: Promise<void> | null = null
  let again = false

  function refetch(): Promise<void> {
    if (inFlight) {
      again = true
      return inFlight
    }
    inFlight = (async () => {
      // Yield first, so `inFlight` is set before the loop can finish.
      await null
      try {
        do {
          again = false
          const project = state.project
          if (project === null) {
            set({ tasks: [], loading: false })
            continue
          }
          try {
            const tasks = await api.listTasks(project)
            // Drop results for a project that is no longer on screen.
            if (state.project === project) set({ tasks, loading: false, error: null })
          } catch (error) {
            if (state.project === project) fail(error)
          }
        } while (again)
      } finally {
        // In the same step as the last check of `again`, so no request
        // can slip in between.
        inFlight = null
      }
    })()
    return inFlight
  }

  function show(project: string | null) {
    if (project === state.project) return refetch()
    set({ project, tasks: [], loading: project !== null, error: null })
    return refetch()
  }

  async function refreshProjects() {
    try {
      set({ projects: await api.listProjects() })
    } catch (error) {
      fail(error)
    }
  }

  return {
    getState: () => state,

    subscribe(listener) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },

    async start() {
      if (unlisten) return
      unlisten = deps.onTasksChanged((event) => {
        if (event.project === state.project) void refetch()
      })
      await unlisten
      // Without them, every feature stays off and the board is as before.
      try {
        set({ features: await api.getFeatures() })
      } catch (error) {
        console.error('could not load the features', error)
      }
      await refreshProjects()
      if (state.project === null) {
        await show(state.projects.find((project) => project.open)?.id ?? null)
      }
    },

    stop() {
      const pending = unlisten
      unlisten = null
      void pending?.then((stop) => stop())
    },

    refreshProjects,

    async openProject(path, allowOutsideGit = false) {
      const opened = await api.openProject(path, allowOutsideGit)
      if (opened.kind === 'project') {
        await refreshProjects()
        await show(opened.project.id)
      }
      return opened
    },

    async removeProject(id) {
      await api.removeProject(id)
      await refreshProjects()
      if (state.project === id) {
        await show(state.projects.find((project) => project.open)?.id ?? null)
      }
    },

    refetch,

    async setFeature(feature, on) {
      set({ features: await api.setFeature(feature, on) })
    },
  }
}

function asCommandError(error: unknown): CommandError {
  if (error instanceof CommandError) return error
  return new CommandError('internal', error instanceof Error ? error.message : String(error))
}

/** The app's board store, backed by the desktop app. */
export const board = createBoardStore()

/**
 * The board state, or the part `select` picks, re-rendering when it
 * changes. `select` must return a value from the state, not a new object.
 */
export function useBoard(): BoardState
export function useBoard<T>(select: (state: BoardState) => T, store?: BoardStore): T
export function useBoard<T>(
  select: (state: BoardState) => T = (state) => state as T,
  store: BoardStore = board,
): T {
  const snapshot = () => select(store.getState())
  return useSyncExternalStore(store.subscribe, snapshot, snapshot)
}

/** Which features are on, re-rendering when they change. */
export function useFeatures(store: BoardStore = board): Features {
  return useBoard((state) => state.features, store)
}

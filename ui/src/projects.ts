// Managing projects from the app: New board, Open board and Remove. The
// flows are plain functions over the folder picker and the store, so they
// can be tested without the desktop app; `useProjectActions` wires them to
// the dialogs.

import { useCallback, useEffect, useState } from 'react'
import { CommandError, type AgentInfo, type Api, type Opened, type ProjectInfo } from './api.ts'
import { errorMessage } from './edit.ts'
import type { BoardStore } from './store.ts'

/** A folder with no board, offered to you before `.tandem/` is created. */
export interface CreateOffer {
  repoPath: string
  /** Whether the folder is in a git repo; if not, the offer warns you. */
  gitRepo: boolean
  /** Whether you asked for a new board, rather than opening a folder. */
  asked: boolean
}

/** What a New board or Open board flow did. */
export type Outcome =
  | { kind: 'cancelled' }
  /** The board is open. `existing` if you asked for a new one and the folder had one. */
  | { kind: 'opened'; project: ProjectInfo; existing: boolean }
  /** Nothing is created until you accept the offer. */
  | { kind: 'offer'; offer: CreateOffer }

export interface FlowDeps {
  /** The system folder picker; `null` if you cancel. */
  chooseFolder: () => Promise<string | null>
  /** Opens a folder and shows it, as `BoardStore.openProject` does. */
  openProject: (path: string, createFolder?: boolean) => Promise<Opened>
}

function opened(result: Opened, existing: boolean): Outcome {
  if (result.kind !== 'project') throw new Error(`could not open ${result.repoPath}`)
  return { kind: 'opened', project: result.project, existing }
}

/**
 * New board: picks a folder and creates `.tandem/` in it. A folder git
 * doesn't track gets an offer with a warning instead, and a folder that
 * already has a board is just opened.
 */
export async function newBoard(deps: FlowDeps): Promise<Outcome> {
  const path = await deps.chooseFolder()
  if (path === null) return { kind: 'cancelled' }
  const result = await deps.openProject(path)
  if (result.kind === 'project') return opened(result, true)
  if (!result.gitRepo) {
    return { kind: 'offer', offer: { repoPath: result.repoPath, gitRepo: false, asked: true } }
  }
  return opened(await deps.openProject(result.repoPath, true), false)
}

/**
 * Open board: opens `path`, or a folder you pick. A folder without a board
 * gets an offer to create one, as the spec's project switcher does.
 */
export async function openBoard(deps: FlowDeps, path?: string): Promise<Outcome> {
  const chosen = path ?? (await deps.chooseFolder())
  if (chosen === null) return { kind: 'cancelled' }
  const result = await deps.openProject(chosen)
  if (result.kind === 'project') return opened(result, false)
  return {
    kind: 'offer',
    offer: { repoPath: result.repoPath, gitRepo: result.gitRepo, asked: false },
  }
}

/** Accepts an offer: creates `.tandem/` and opens the board. */
export async function createBoard(deps: FlowDeps, offer: CreateOffer): Promise<Outcome> {
  return opened(await deps.openProject(offer.repoPath, true), false)
}

/** A task an agent is working on now. */
export interface WorkingTask {
  id: string
  title: string
}

/** The tasks agents hold a live lease on, which block removing the project. */
export function workingTasks(agents: readonly AgentInfo[]): WorkingTask[] {
  return agents
    .flatMap((agent) => agent.tasks)
    .filter((task) => task.live)
    .map(({ id, title }) => ({ id, title }))
    .sort((a, b) => a.id.localeCompare(b.id))
}

/** A short label for why a listed project isn't open. */
export function problemLabel(error: string): string {
  if (error === 'the folder is missing') return 'folder missing'
  if (error.startsWith('the folder has no ')) return 'no board'
  return "can't open"
}

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' })

/** When a project was last opened, such as `2 hours ago`. */
export function openedAgo(iso: string | null, now: number): string {
  if (iso === null) return 'never opened'
  const then = Date.parse(iso)
  if (Number.isNaN(then)) return ''
  const minutes = Math.round((now - then) / 60_000)
  if (minutes < 1) return 'just now'
  if (minutes < 60) return relative.format(-minutes, 'minute')
  const hours = Math.round(minutes / 60)
  if (hours < 24) return relative.format(-hours, 'hour')
  const days = Math.round(hours / 24)
  if (days < 30) return relative.format(-days, 'day')
  return new Date(then).toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
  })
}

/**
 * The project actions and the state of their dialogs. Errors from the
 * picker or opening go to `onError`; outcomes worth a word go to `onInfo`.
 * `tasks` is the board's task list: a change rechecks whether agents are
 * still working on the project being removed.
 */
export function useProjectActions({
  store,
  api,
  tasks,
  onError,
  onInfo,
}: {
  store: Pick<BoardStore, 'openProject' | 'removeProject'>
  api: Pick<Api, 'chooseFolder' | 'listAgents'>
  tasks: unknown
  onError: (message: string) => void
  onInfo: (message: string | null) => void
}) {
  const [offer, setOffer] = useState<CreateOffer | null>(null)
  const [removing, setRemoving] = useState<ProjectInfo | null>(null)
  // `null` while the first check runs.
  const [working, setWorking] = useState<WorkingTask[] | null>(null)
  const [checks, setChecks] = useState(0)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const deps: FlowDeps = { chooseFolder: api.chooseFolder, openProject: store.openProject }

  function settle(outcome: Outcome) {
    if (outcome.kind === 'offer') {
      setError(null)
      setOffer(outcome.offer)
    } else if (outcome.kind === 'opened' && outcome.existing) {
      onInfo(`${outcome.project.repoPath} already has a board, so Tandem opened it.`)
    }
  }

  async function start(flow: () => Promise<Outcome>) {
    onInfo(null)
    try {
      settle(await flow())
    } catch (error) {
      onError(errorMessage(error))
    }
  }

  async function accept() {
    if (offer === null) return
    setBusy(true)
    setError(null)
    try {
      await createBoard(deps, offer)
      setOffer(null)
    } catch (error) {
      setError(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  function askRemove(project: ProjectInfo) {
    onInfo(null)
    setError(null)
    // A project that isn't open has no agents to wait for.
    setWorking(project.open ? null : [])
    setRemoving(project)
  }

  async function remove() {
    if (removing === null) return
    setBusy(true)
    setError(null)
    try {
      await store.removeProject(removing.id)
      onInfo(`Removed ${removing.name} from Tandem. Its files are still in ${removing.repoPath}.`)
      setRemoving(null)
    } catch (error) {
      setError(errorMessage(error))
      if (error instanceof CommandError && error.code === 'agents_working') {
        setChecks((n) => n + 1)
      }
    } finally {
      setBusy(false)
    }
  }

  // Which tasks agents are working on in the project being removed, again
  // whenever the board's tasks change or you ask.
  const listAgents = api.listAgents
  useEffect(() => {
    if (removing === null || !removing.open) return
    let current = true
    listAgents(removing.id).then(
      (agents) => current && setWorking(workingTasks(agents)),
      // The remove command checks again, so a failed check doesn't block.
      () => current && setWorking([]),
    )
    return () => {
      current = false
    }
  }, [listAgents, removing, checks, tasks])

  return {
    newBoard: () => void start(() => newBoard(deps)),
    openBoard: (path?: string) => void start(() => openBoard(deps, path)),
    offer,
    accept: () => void accept(),
    closeOffer: useCallback(() => setOffer(null), []),
    removing,
    working,
    askRemove,
    remove: () => void remove(),
    recheck: () => setChecks((n) => n + 1),
    closeRemove: useCallback(() => setRemoving(null), []),
    busy,
    error,
  }
}

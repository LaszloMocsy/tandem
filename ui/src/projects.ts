// Managing projects from the app: Open folder, switching and Remove. The
// flows are plain functions over the folder picker and the store, so they
// can be tested without the desktop app; `useProjectActions` wires them to
// the dialogs.

import { useCallback, useEffect, useState } from 'react'
import { CommandError, type AgentInfo, type Api, type Opened, type ProjectInfo } from './api.ts'
import { errorMessage } from './edit.ts'
import type { BoardStore } from './store.ts'

/** What an Open folder flow did. */
export type Outcome =
  | { kind: 'cancelled' }
  | { kind: 'opened'; project: ProjectInfo }
  /** The folder is not in a git repo; nothing is added until you confirm. */
  | { kind: 'outsideGit'; repoPath: string }

export interface FlowDeps {
  /** The system folder picker; `null` if you cancel. */
  chooseFolder: () => Promise<string | null>
  /** Opens a folder and shows it, as `BoardStore.openProject` does. */
  openProject: (path: string, allowOutsideGit?: boolean) => Promise<Opened>
}

function outcome(result: Opened): Outcome {
  return result.kind === 'project'
    ? { kind: 'opened', project: result.project }
    : { kind: 'outsideGit', repoPath: result.repoPath }
}

/**
 * Open folder: opens `path`, or a folder you pick. A folder in a git repo
 * opens its repo's board, starting an empty one the first time. A folder
 * outside git needs your confirmation first, unless `allowOutsideGit`, as
 * for a project already in the list.
 */
export async function openFolder(
  deps: FlowDeps,
  path?: string,
  allowOutsideGit = false,
): Promise<Outcome> {
  const chosen = path ?? (await deps.chooseFolder())
  if (chosen === null) return { kind: 'cancelled' }
  return outcome(await deps.openProject(chosen, allowOutsideGit))
}

/** Confirms a folder outside git: it becomes a project of its own. */
export async function openOutsideGit(deps: FlowDeps, repoPath: string): Promise<Outcome> {
  return outcome(await deps.openProject(repoPath, true))
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
  if (error === 'the project path is not a folder') return 'not a folder'
  if (error === 'the project is not open') return 'not open'
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
  // A folder outside git, waiting for you to confirm it.
  const [outsideGit, setOutsideGit] = useState<string | null>(null)
  const [removing, setRemoving] = useState<ProjectInfo | null>(null)
  // `null` while the first check runs.
  const [working, setWorking] = useState<WorkingTask[] | null>(null)
  const [checks, setChecks] = useState(0)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const deps: FlowDeps = { chooseFolder: api.chooseFolder, openProject: store.openProject }

  async function start(flow: () => Promise<Outcome>) {
    onInfo(null)
    try {
      const outcome = await flow()
      if (outcome.kind === 'outsideGit') {
        setError(null)
        setOutsideGit(outcome.repoPath)
      }
    } catch (error) {
      onError(errorMessage(error))
    }
  }

  async function confirmOutsideGit() {
    if (outsideGit === null) return
    setBusy(true)
    setError(null)
    try {
      await openOutsideGit(deps, outsideGit)
      setOutsideGit(null)
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
      onInfo(`Removed ${removing.name} from Tandem. Its folder is still in ${removing.repoPath}.`)
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
    openFolder: () => void start(() => openFolder(deps)),
    /** Shows a project from the list, which you confirmed when you added it. */
    switchTo: (project: ProjectInfo) => void start(() => openFolder(deps, project.repoPath, true)),
    outsideGit,
    confirmOutsideGit: () => void confirmOutsideGit(),
    closeOutsideGit: useCallback(() => setOutsideGit(null), []),
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

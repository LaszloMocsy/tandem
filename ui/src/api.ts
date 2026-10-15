// The typed client for the desktop app's Tauri commands and events. The
// shapes mirror `desktop/src/dto.rs` and `desktop/src/projects.rs`.

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export type Status = 'backlog' | 'todo' | 'in_progress' | 'finished' | 'done'
export type Priority = 'high' | 'medium' | 'low'

export interface ProjectInfo {
  id: string
  name: string
  repoPath: string
  lastOpenedAt: string | null
  /** Whether the project is open, so its tasks can be listed. */
  open: boolean
  /** Why the project is not open, when it isn't. */
  error: string | null
  /** Why the project is read-only, when it is. */
  readOnly: string | null
  /** The project's MCP endpoint, while it is open and the server runs. */
  mcpUrl: string | null
}

/** What opening a folder did. */
export type Opened =
  | { kind: 'project'; project: ProjectInfo }
  /** The folder has no `.tandem/`; open it with `createFolder` to make one. */
  | { kind: 'noTaskFolder'; repoPath: string }

export interface Lease {
  agentId: string
  claimedAt: string
  expiresAt: string
  /** `false` once the lease has expired and another agent can take over. */
  live: boolean
}

export interface TaskProblem {
  /** `graph` is a loop or a missing parent. */
  kind: 'invalid_file' | 'graph'
  message: string
}

/** An acceptance criterion the agent left unmet, and why. */
export interface UnmetCriterion {
  /** Counting from 1. */
  criterion: number
  text: string
  reason: string
}

export interface TaskSummary {
  id: string
  title: string
  status: Status
  priority: Priority
  parent: string | null
  labels: string[]
  dependsOn: string[]
  /** `YYYY-MM-DD`; `null` only if the file is invalid. */
  created: string | null
  lease: Lease | null
  blocked: string | null
  problem: TaskProblem | null
  /** Only filled in for tasks in `finished`. */
  unmet: UnmetCriterion[]
}

export interface WorkLogEntry {
  /** `null` means you. */
  agentId: string | null
  kind: 'note' | 'blocked' | 'unblocked' | 'feedback' | 'event'
  message: string
  createdAt: string
}

export interface Criterion {
  /** Counting from 1. */
  number: number
  met: boolean
  text: string
}

/** One changelog entry, as read back from the file. */
export interface ChangelogEntry {
  date: string
  who: string
  event: string
  /** The summary and any `Files:`, `Follow-ups:` and `Unmet:` lines. */
  text: string
}

/** The parsed task file. */
export interface TaskContent {
  /** Empty if there is no Description section. */
  description: string
  criteria: Criterion[]
  relevantFiles: string[]
  /** Oldest first. */
  changelog: ChangelogEntry[]
  reviewedAt: string | null
}

export interface TaskDetail {
  summary: TaskSummary
  /** The task file, as it is on disk. */
  markdown: string
  parent: TaskSummary | null
  children: TaskSummary[]
  /** Oldest first. */
  workLog: WorkLogEntry[]
  reviewFeedback: string | null
  /** `null` if the file is invalid. */
  content: TaskContent | null
}

/** The file a write changed. */
export interface Written {
  id: string
  /** Relative to the repo root, such as `.tandem/k3f9x2m.md`. */
  path: string
  status: Status
}

export interface TaskFilter {
  status?: Status
  label?: string
  parent?: string
  /** Matches the id, title, labels or body, ignoring case. */
  query?: string
  readyOnly?: boolean
}

export interface NewTask {
  title: string
  description?: string
  acceptanceCriteria?: string[]
  priority?: Priority
  parent?: string
  labels?: string[]
  dependsOn?: string[]
}

/** The fields to change; a missing field stays as it is. */
export interface TaskEdit {
  title?: string
  /** A blank value removes the section. */
  description?: string
  /** Replaces every criterion, unticked. */
  acceptanceCriteria?: string[]
  relevantFiles?: string[]
  priority?: Priority
  /** `null` moves the task to the top level. */
  parent?: string | null
  labels?: string[]
  dependsOn?: string[]
}

export interface Finish {
  summary: string
  filesChanged?: string[]
  followUps?: string
  /** Acceptance criteria met, by number from 1. */
  met?: number[]
  /** Every criterion still unticked, with the reason. */
  unmet?: { criterion: number; reason: string }[]
}

/** The payload of the `tasks-changed` event. */
export interface TasksChanged {
  project: string
  changed: string[]
  removed: string[]
}

export const TASKS_CHANGED = 'tasks-changed'

/**
 * A refused command. `code` is one of the spec's error codes, such as
 * `invalid_transition` or `cycle_detected`, or `project_not_found`,
 * `invalid_argument`, `read_only` or `internal`.
 */
export class CommandError extends Error {
  readonly code: string

  constructor(code: string, message: string) {
    super(message)
    this.name = 'CommandError'
    this.code = code
  }
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args)
  } catch (error) {
    throw toCommandError(error)
  }
}

function toCommandError(error: unknown): CommandError {
  if (error instanceof CommandError) return error
  if (
    typeof error === 'object' &&
    error !== null &&
    typeof (error as { code?: unknown }).code === 'string' &&
    typeof (error as { message?: unknown }).message === 'string'
  ) {
    const { code, message } = error as { code: string; message: string }
    return new CommandError(code, message)
  }
  return new CommandError('internal', error instanceof Error ? error.message : String(error))
}

/** Every command the desktop app offers. Task commands act as you. */
export const api = {
  /** Every project, the most recently opened first. */
  listProjects: () => call<ProjectInfo[]>('list_projects'),
  /**
   * Opens a repo folder and makes it the most recent project. Without
   * `createFolder`, a folder with no `.tandem/` is left alone.
   */
  openProject: (path: string, createFolder = false) =>
    call<Opened>('open_project', { path, createFolder }),
  /** Closes a project and removes it from the recent list. */
  removeProject: (project: string) => call<boolean>('remove_project', { project }),
  /** Asks for a folder with the system dialog; `null` if cancelled. */
  chooseFolder: () => call<string | null>('choose_folder'),

  listTasks: (project: string, filter?: TaskFilter) =>
    call<TaskSummary[]>('list_tasks', { project, filter }),
  getTask: (project: string, id: string) => call<TaskDetail>('get_task', { project, id }),
  createTask: (project: string, task: NewTask) =>
    call<Written>('create_task', { project, task }),
  updateTask: (project: string, id: string, edit: TaskEdit) =>
    call<Written>('update_task', { project, id, edit }),

  /** Backlog → Todo. */
  approve: (project: string, id: string) => call<Written>('approve_task', { project, id }),
  /** Todo → In progress. */
  start: (project: string, id: string) => call<Written>('start_task', { project, id }),
  /** In progress → Review. */
  finish: (project: string, id: string, finish: Finish) =>
    call<Written>('finish_task', { project, id, finish }),
  /** In progress → Todo. */
  release: (project: string, id: string, note = '') =>
    call<Written>('release_task', { project, id, note }),
  /** Review → Done. */
  markReviewed: (project: string, id: string) =>
    call<Written>('mark_reviewed', { project, id }),
  /** Review → In progress, with feedback for the next agent. */
  reopen: (project: string, id: string, feedback: string) =>
    call<Written>('reopen_task', { project, id, feedback }),

  /** Sets the blocked flag, or clears it when `reason` is blank. */
  setBlocked: (project: string, id: string, reason: string) =>
    call<null>('set_blocked', { project, id, reason }),
  logProgress: (project: string, id: string, message: string) =>
    call<null>('log_progress', { project, id, message }),
  addChangelogEntry: (project: string, id: string, summary: string, files: string[] = []) =>
    call<Written>('add_changelog_entry', { project, id, summary, files }),
}

export type Api = typeof api

/** Calls `handler` for every `tasks-changed` event, until unlistened. */
export function onTasksChanged(handler: (event: TasksChanged) => void): Promise<UnlistenFn> {
  return listen<TasksChanged>(TASKS_CHANGED, (event) => handler(event.payload))
}

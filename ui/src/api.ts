// The typed client for the desktop app's Tauri commands and events. The
// shapes mirror `desktop/src/dto.rs` and `desktop/src/projects.rs`.

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export type Status = 'backlog' | 'todo' | 'in_progress' | 'finished' | 'done'
export type Priority = 'high' | 'medium' | 'low'

export interface ProjectInfo {
  id: string
  name: string
  /** The checkout or folder last opened. */
  repoPath: string
  lastOpenedAt: string | null
  /** Whether the project is open, so its tasks can be listed. */
  open: boolean
  /** Why the project is not open, when it isn't. */
  error: string | null
  /** The project's MCP endpoint, while it is open and the server runs. */
  mcpUrl: string | null
}

export type Theme = 'system' | 'light' | 'dark'

/** A page the app opens in the browser; the desktop app builds its URL. */
export type Link = 'repository' | 'releaseNotes'

/** The app's settings, for every project on this machine. */
export interface AppSettings {
  /** The port the MCP server is set to use. */
  mcpPort: number
  defaultMcpPort: number
  /** Where the server listens, such as `http://127.0.0.1:4317`, while it runs. */
  mcpUrl: string | null
  /** Why the server isn't running, when it isn't. */
  mcpError: string | null
  theme: Theme
}

/** What opening a folder did. */
export type Opened =
  | { kind: 'project'; project: ProjectInfo }
  /**
   * The folder is not in a git repo. Nothing was added; open it again with
   * `allowOutsideGit` to make it a project of its own.
   */
  | { kind: 'outsideGit'; repoPath: string }

/** What stands in the way of deleting a task. */
export interface DeleteCheck {
  /** Tasks whose parent is this task. */
  children: string[]
  /** Tasks that depend on this task. */
  dependents: string[]
  /** The live lease, while an agent is working on the task. */
  lease: Lease | null
}

/** A deleted task, which can still be restored until deleted tasks are cleaned up. */
export interface DeletedTask {
  id: string
  title: string
  status: Status
  deletedAt: string
}

export interface Lease {
  agentId: string
  claimedAt: string
  expiresAt: string
  /** `false` once the lease has expired and another agent can take over. */
  live: boolean
}

/** An agent in the Agents panel. */
export interface AgentInfo {
  agentId: string
  /** From MCP `clientInfo`, such as `claude-code`. */
  clientName: string | null
  lastSeenAt: string
  /** The tasks it holds a lease on. */
  tasks: { id: string; title: string; live: boolean }[]
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
  /** `YYYY-MM-DD`. */
  created: string
  lease: Lease | null
  blocked: string | null
  /** Only filled in for tasks in `finished`. */
  unmet: UnmetCriterion[]
}

export interface WorkLogEntry {
  /** `null` means you. */
  agentId: string | null
  kind: 'note' | 'blocked' | 'unblocked' | 'feedback'
  message: string
  createdAt: string
}

export interface Criterion {
  /** Counting from 1. */
  number: number
  met: boolean
  text: string
}

/** One changelog entry. */
export interface ChangelogEntry {
  date: string
  who: string
  event: string
  /** The summary and any `Files:`, `Follow-ups:` and `Unmet:` lines. */
  text: string
}

/** The task's spec and changelog. */
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
  /** The task rendered as Markdown. */
  markdown: string
  parent: TaskSummary | null
  children: TaskSummary[]
  /** Oldest first. */
  workLog: WorkLogEntry[]
  reviewFeedback: string | null
  content: TaskContent
}

/** The task a change saved, and its status after it. */
export interface Saved {
  id: string
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
  /**
   * Replaces every criterion. Unchanged ticked ones stay ticked; new or
   * reworded ones start unticked.
   */
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
  /** Tasks deleted or cleaned up. */
  removed: string[]
}

export const TASKS_CHANGED = 'tasks-changed'

/**
 * A refused command. `code` is one of the service's error codes, such as
 * `invalid_transition` or `cycle_detected`, or `project_not_found`,
 * `invalid_argument`, `agents_working`, `task_referenced`, `port_in_use`,
 * `mcp_failed` or `internal`.
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
   * Opens a folder and makes it the most recent project. A folder in a git
   * repo opens its repo's project; one outside git is left alone unless
   * `allowOutsideGit` is set.
   */
  openProject: (path: string, allowOutsideGit = false) =>
    call<Opened>('open_project', { path, allowOutsideGit }),
  /**
   * Closes a project, removes it from the recent list and deletes its
   * database, with every task and its history. The folder stays. Refused
   * with `agents_working` while an agent holds a live lease on one of its
   * tasks.
   */
  removeProject: (project: string) => call<boolean>('remove_project', { project }),
  /** Asks for a folder with the system dialog; `null` if cancelled. */
  chooseFolder: () => call<string | null>('choose_folder'),
  /** Opens the repository or the running version's release notes in the browser. */
  openLink: (link: Link) => call<null>('open_link', { link }),
  /**
   * Renames the project on this machine; a blank name goes back to the
   * folder name.
   */
  setProjectName: (project: string, name: string) =>
    call<ProjectInfo>('set_project_name', { project, name }),

  getAppSettings: () => call<AppSettings>('get_app_settings'),
  /**
   * Moves the MCP server to `port`, changing every project's MCP URL.
   * Refused with `port_in_use` if the port is taken; the server then stays
   * where it was.
   */
  setMcpPort: (port: number) => call<AppSettings>('set_mcp_port', { port }),
  setTheme: (theme: Theme) => call<AppSettings>('set_theme', { theme }),

  listTasks: (project: string, filter?: TaskFilter) =>
    call<TaskSummary[]>('list_tasks', { project, filter }),
  getTask: (project: string, id: string) => call<TaskDetail>('get_task', { project, id }),
  /** Agents seen in the last day, and any still holding a lease, newest first. */
  listAgents: (project: string) => call<AgentInfo[]>('list_agents', { project }),
  createTask: (project: string, task: NewTask) =>
    call<Saved>('create_task', { project, task }),
  updateTask: (project: string, id: string, edit: TaskEdit) =>
    call<Saved>('update_task', { project, id, edit }),
  /** What stands in the way of deleting a task. */
  deleteCheck: (project: string, id: string) =>
    call<DeleteCheck>('delete_check', { project, id }),
  /**
   * Deletes a task, at any status, until it is restored or cleaned up.
   * Refused with `claimed_by_other` while an agent works on it, and
   * `task_referenced` while another task links to it.
   */
  deleteTask: (project: string, id: string) => call<null>('delete_task', { project, id }),
  /** The deleted tasks, which can still be restored, newest first. */
  listDeletedTasks: (project: string) =>
    call<DeletedTask[]>('list_deleted_tasks', { project }),
  /**
   * Brings a deleted task back. Refused with `parent_not_found` or
   * `not_found` while its parent or a dependency is deleted too.
   */
  restoreTask: (project: string, id: string) => call<null>('restore_task', { project, id }),
  /**
   * Removes every deleted task for good, so none can be restored. Returns
   * how many went.
   */
  purgeDeletedTasks: (project: string) => call<number>('purge_deleted_tasks', { project }),

  /** Backlog → Todo. */
  approve: (project: string, id: string) => call<Saved>('approve_task', { project, id }),
  /** Todo → In progress. */
  start: (project: string, id: string) => call<Saved>('start_task', { project, id }),
  /** In progress → Review. */
  finish: (project: string, id: string, finish: Finish) =>
    call<Saved>('finish_task', { project, id, finish }),
  /** In progress → Todo. */
  release: (project: string, id: string, note = '') =>
    call<Saved>('release_task', { project, id, note }),
  /** Review → Done. */
  markReviewed: (project: string, id: string) =>
    call<Saved>('mark_reviewed', { project, id }),
  /** Review → In progress, with feedback for the next agent. */
  reopen: (project: string, id: string, feedback: string) =>
    call<Saved>('reopen_task', { project, id, feedback }),

  /** Sets the blocked flag, or clears it when `reason` is blank. */
  setBlocked: (project: string, id: string, reason: string) =>
    call<null>('set_blocked', { project, id, reason }),
  logProgress: (project: string, id: string, message: string) =>
    call<null>('log_progress', { project, id, message }),
  addChangelogEntry: (project: string, id: string, summary: string, files: string[] = []) =>
    call<Saved>('add_changelog_entry', { project, id, summary, files }),
}

export type Api = typeof api

/** Calls `handler` for every `tasks-changed` event, until unlistened. */
export function onTasksChanged(handler: (event: TasksChanged) => void): Promise<UnlistenFn> {
  return listen<TasksChanged>(TASKS_CHANGED, (event) => handler(event.payload))
}

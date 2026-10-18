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

export type Theme = 'system' | 'light' | 'dark'

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
   * The folder has no `.tandem/`; open it with `createFolder` to make one.
   * `gitRepo` says whether the folder is in a git repo.
   */
  | { kind: 'noTaskFolder'; repoPath: string; gitRepo: boolean }

/** What stands in the way of deleting a task, and what deleting it loses. */
export interface DeleteCheck {
  /** Tasks whose parent is this task. */
  children: string[]
  /** Tasks that depend on this task. */
  dependents: string[]
  /** The live lease, while an agent is working on the task. */
  lease: Lease | null
  /** Whether git history keeps a copy of the file. */
  committed: boolean
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
  removed: string[]
}

export const TASKS_CHANGED = 'tasks-changed'

/** The payload of the `projects-changed` event, such as after a rename. */
export interface ProjectsChanged {
  project: string
}

export const PROJECTS_CHANGED = 'projects-changed'

/**
 * A refused command. `code` is one of the spec's error codes, such as
 * `invalid_transition` or `cycle_detected`, or `project_not_found`,
 * `invalid_argument`, `read_only`, `git_failed`, `agents_working`,
 * `task_referenced`, `port_in_use`, `mcp_failed` or `internal`.
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
  /**
   * Closes a project, removes it from the recent list and deletes its local
   * database. The repo and its `.tandem/` files stay. Refused with
   * `agents_working` while an agent holds a live lease on one of its tasks.
   */
  removeProject: (project: string) => call<boolean>('remove_project', { project }),
  /** Asks for a folder with the system dialog; `null` if cancelled. */
  chooseFolder: () => call<string | null>('choose_folder'),
  /**
   * Sets the project's name in `.tandem/config.toml`; a blank name removes
   * it, so the folder name is used. Refused with `read_only` on a read-only
   * project.
   */
  setProjectName: (project: string, name: string) =>
    call<ProjectInfo>('set_project_name', { project, name }),
  /**
   * Reset local state: deletes the project's database and rebuilds it from
   * the task files. Leases, flags and working logs are lost.
   */
  resetProject: (project: string) => call<null>('reset_project', { project }),

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
    call<Written>('create_task', { project, task }),
  updateTask: (project: string, id: string, edit: TaskEdit) =>
    call<Written>('update_task', { project, id, edit }),
  /** What stands in the way of deleting a task. */
  deleteCheck: (project: string, id: string) =>
    call<DeleteCheck>('delete_check', { project, id }),
  /**
   * Deletes a task's file, at any status. Refused with `claimed_by_other`
   * while an agent works on it, and `task_referenced` while another task
   * links to it.
   */
  deleteTask: (project: string, id: string) => call<null>('delete_task', { project, id }),

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
  /** Whether the task file has changes git hasn't committed. */
  hasUncommittedChanges: (project: string, id: string) =>
    call<boolean>('has_uncommitted_changes', { project, id }),
  /** Commits a Done task's file and nothing else; returns the commit hash. */
  commitReview: (project: string, id: string) =>
    call<string>('commit_review', { project, id }),

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

/** Calls `handler` for every `projects-changed` event, until unlistened. */
export function onProjectsChanged(
  handler: (event: ProjectsChanged) => void,
): Promise<UnlistenFn> {
  return listen<ProjectsChanged>(PROJECTS_CHANGED, (event) => handler(event.payload))
}

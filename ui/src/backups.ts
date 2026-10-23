// Backups in Settings → Project: listing them, Back up now, Restore behind
// its confirmation, and Export. The logic is a plain object over the api,
// so it can be tested without the desktop app; `useBackups` wires it to
// React.

import { useEffect, useState, useSyncExternalStore } from 'react'
import type { Api, BackupInfo, ExportFormat } from './api.ts'
import { errorMessage, formatTime } from './edit.ts'

export type BackupsApi = Pick<
  Api,
  'listBackups' | 'backUpProject' | 'restoreBackup' | 'exportTasks'
>

export interface BackupsState {
  /** Newest first; `null` until the first load. */
  backups: BackupInfo[] | null
  loadError: string | null
  /** The backup the Restore confirmation is open for. */
  confirming: BackupInfo | null
  busy: boolean
  /** What the latest action did, such as where an export went. */
  notice: string | null
  /** The latest refused backup or export. */
  error: string | null
  /** The latest refused restore, shown in its dialog. */
  restoreError: string | null
}

const initialState: BackupsState = {
  backups: null,
  loadError: null,
  confirming: null,
  busy: false,
  notice: null,
  error: null,
  restoreError: null,
}

export function createBackups(api: BackupsApi, project: string) {
  let state = initialState
  const listeners = new Set<() => void>()
  // Drops the results of a load that a newer one overtook.
  let loads = 0

  function set(patch: Partial<BackupsState>) {
    state = { ...state, ...patch }
    for (const listener of listeners) listener()
  }

  async function load() {
    const load = ++loads
    try {
      const backups = await api.listBackups(project)
      if (load === loads) set({ backups, loadError: null })
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

    async backUpNow() {
      set({ busy: true, error: null, notice: null })
      try {
        const backup = await api.backUpProject(project)
        set({ notice: `Backed up ${backupLabel(backup)}.` })
      } catch (error) {
        set({ error: errorMessage(error) })
      } finally {
        set({ busy: false })
        await load()
      }
    },

    /** Opens the confirmation; nothing is restored yet. */
    askRestore(backup: BackupInfo) {
      set({ confirming: backup, restoreError: null, error: null, notice: null })
    },

    cancelRestore() {
      set({ confirming: null })
    },

    /** Restores the backup the confirmation is open for. */
    async confirmRestore() {
      const backup = state.confirming
      if (backup === null) return
      set({ busy: true, restoreError: null })
      try {
        const undo = await api.restoreBackup(project, backup.name)
        set({
          confirming: null,
          notice: `Restored the backup from ${formatTime(backup.createdAt)}. To undo it, restore the backup from ${formatTime(undo.createdAt)}.`,
        })
      } catch (error) {
        set({ restoreError: errorMessage(error) })
      } finally {
        set({ busy: false })
        await load()
      }
    },

    async exportTasks(format: ExportFormat) {
      set({ busy: true, error: null, notice: null })
      try {
        const path = await api.exportTasks(project, format)
        if (path !== null) set({ notice: `Exported to ${path}.` })
      } catch (error) {
        set({ error: errorMessage(error) })
      } finally {
        set({ busy: false })
      }
    },
  }
}

export type Backups = ReturnType<typeof createBackups>

/**
 * The backups of `project` and their actions. `tasks` is the board's task
 * list: a change loads the list again, since the app backs up changed
 * projects in the background.
 */
export function useBackups(api: BackupsApi, project: string, tasks: unknown) {
  const [backups] = useState(() => createBackups(api, project))
  const state = useSyncExternalStore(backups.subscribe, backups.getState, backups.getState)
  useEffect(() => {
    void backups.load()
  }, [backups, tasks])
  return { ...state, actions: backups }
}

/** A backup's time and tasks, such as `Oct 23, 09:30 · 12 tasks`. */
export function backupLabel(backup: BackupInfo): string {
  return `${formatTime(backup.createdAt)} · ${taskCount(backup.tasks)}`
}

export function taskCount(tasks: number | null): string {
  if (tasks === null) return 'unreadable'
  return tasks === 1 ? '1 task' : `${tasks} tasks`
}

/** A file size in bytes, KB or MB. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

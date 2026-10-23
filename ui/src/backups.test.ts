import { expect, test } from 'vitest'
import { type BackupInfo, CommandError } from './api.ts'
import { type BackupsApi, createBackups, formatSize, taskCount } from './backups.ts'

function backup(name: string, tasks: number | null = 1): BackupInfo {
  return { name, createdAt: '2026-10-23T09:00:00.000Z', size: 4096, tasks }
}

/** A fake api over one project's backups, recording every call. */
function fake(backups: BackupInfo[], { working = false, exported = '/tmp/demo.json' as string | null } = {}) {
  const calls: string[] = []
  const api: BackupsApi = {
    listBackups: async (project) => {
      calls.push(`list ${project}`)
      return [...backups]
    },
    backUpProject: async (project) => {
      calls.push(`back up ${project}`)
      const made = backup(`new-${backups.length}`)
      backups = [made, ...backups]
      return made
    },
    restoreBackup: async (project, name) => {
      calls.push(`restore ${project} ${name}`)
      if (working) throw new CommandError('agents_working', 'an agent is working on task aaaaaaa')
      const undo = backup('undo')
      backups = [undo, ...backups]
      return undo
    },
    exportTasks: async (project, format) => {
      calls.push(`export ${project} ${format}`)
      return exported
    },
  }
  return { api, calls }
}

test('loads the backups, newest first as the app sends them', async () => {
  const app = fake([backup('b'), backup('a')])
  const model = createBackups(app.api, 'demo')
  expect(model.getState().backups).toBeNull()
  await model.load()
  expect(model.getState().backups?.map((b) => b.name)).toEqual(['b', 'a'])
})

test('Back up now adds a backup and reloads the list', async () => {
  const app = fake([backup('a')])
  const model = createBackups(app.api, 'demo')
  await model.backUpNow()
  expect(app.calls).toEqual(['back up demo', 'list demo'])
  expect(model.getState().backups?.map((b) => b.name)).toEqual(['new-1', 'a'])
  expect(model.getState().notice).toMatch(/^Backed up .* · 1 task\.$/)
})

test('Restore asks first, then restores and says how to undo it', async () => {
  const app = fake([backup('a')])
  const model = createBackups(app.api, 'demo')
  await model.load()
  await model.confirmRestore()
  expect(app.calls).toEqual(['list demo'])

  model.askRestore(backup('a'))
  expect(model.getState().confirming?.name).toBe('a')
  model.cancelRestore()
  expect(model.getState().confirming).toBeNull()

  model.askRestore(backup('a'))
  await model.confirmRestore()
  expect(app.calls).toEqual(['list demo', 'restore demo a', 'list demo'])
  expect(model.getState()).toMatchObject({ confirming: null, busy: false, restoreError: null })
  expect(model.getState().notice).toMatch(/^Restored the backup from .*To undo it, restore the backup from/)
})

test('a refused restore keeps the dialog open and says why', async () => {
  const app = fake([backup('a')], { working: true })
  const model = createBackups(app.api, 'demo')
  model.askRestore(backup('a'))
  await model.confirmRestore()
  expect(model.getState()).toMatchObject({
    confirming: backup('a'),
    restoreError: 'an agent is working on task aaaaaaa',
    busy: false,
  })
})

test('Export says where it went, and a cancelled export says nothing', async () => {
  const model = createBackups(fake([]).api, 'demo')
  await model.exportTasks('json')
  expect(model.getState().notice).toBe('Exported to /tmp/demo.json.')
  const cancelled = createBackups(fake([], { exported: null }).api, 'demo')
  await cancelled.exportTasks('markdown')
  expect(cancelled.getState().notice).toBeNull()
})

test('sizes and task counts read naturally', () => {
  expect(formatSize(900)).toBe('900 B')
  expect(formatSize(40960)).toBe('40 KB')
  expect(formatSize(3 * 1024 * 1024)).toBe('3.0 MB')
  expect(taskCount(1)).toBe('1 task')
  expect(taskCount(0)).toBe('0 tasks')
  expect(taskCount(null)).toBe('unreadable')
})

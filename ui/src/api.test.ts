import { invoke } from '@tauri-apps/api/core'
import { beforeEach, expect, test, vi } from 'vitest'
import { api, CommandError } from './api.ts'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

const mockedInvoke = vi.mocked(invoke)

beforeEach(() => {
  mockedInvoke.mockReset()
  mockedInvoke.mockResolvedValue(null)
})

test('commands pass camelCase arguments under the Rust names', async () => {
  await api.openProject('/repo')
  await api.openProject('/repo', true)
  await api.listTasks('demo', { status: 'todo', readyOnly: true })
  await api.updateTask('demo', 'k3f9x2m', { parent: null, relevantFiles: ['a.rs'] })
  await api.approve('demo', 'k3f9x2m')
  await api.markReviewed('demo', 'k3f9x2m')
  await api.reopen('demo', 'k3f9x2m', 'Missing a test.')
  await api.release('demo', 'k3f9x2m')
  await api.listDeletedTasks('demo')
  await api.restoreTask('demo', 'k3f9x2m')
  await api.purgeDeletedTasks('demo')
  await api.listBackups('demo')
  await api.backUpProject('demo')
  await api.restoreBackup('demo', '20261023T093000.123Z')
  await api.exportTasks('demo', 'markdown')
  expect(mockedInvoke.mock.calls).toEqual([
    ['open_project', { path: '/repo', allowOutsideGit: false }],
    ['open_project', { path: '/repo', allowOutsideGit: true }],
    ['list_tasks', { project: 'demo', filter: { status: 'todo', readyOnly: true } }],
    [
      'update_task',
      { project: 'demo', id: 'k3f9x2m', edit: { parent: null, relevantFiles: ['a.rs'] } },
    ],
    ['approve_task', { project: 'demo', id: 'k3f9x2m' }],
    ['mark_reviewed', { project: 'demo', id: 'k3f9x2m' }],
    ['reopen_task', { project: 'demo', id: 'k3f9x2m', feedback: 'Missing a test.' }],
    ['release_task', { project: 'demo', id: 'k3f9x2m', note: '' }],
    ['list_deleted_tasks', { project: 'demo' }],
    ['restore_task', { project: 'demo', id: 'k3f9x2m' }],
    ['purge_deleted_tasks', { project: 'demo' }],
    ['list_backups', { project: 'demo' }],
    ['back_up_project', { project: 'demo' }],
    ['restore_backup', { project: 'demo', name: '20261023T093000.123Z' }],
    ['export_tasks', { project: 'demo', format: 'markdown' }],
  ])
})

test('refusals become CommandErrors with the code', async () => {
  mockedInvoke.mockRejectedValueOnce({ code: 'invalid_transition', message: 'cannot approve' })
  const error = await api.approve('demo', 'k3f9x2m').catch((e: unknown) => e)
  expect(error).toBeInstanceOf(CommandError)
  expect(error).toMatchObject({ code: 'invalid_transition', message: 'cannot approve' })

  mockedInvoke.mockRejectedValueOnce('command list_tasks not found')
  await expect(api.listTasks('demo')).rejects.toMatchObject({
    code: 'internal',
    message: 'command list_tasks not found',
  })
})

import { expect, test } from 'vitest'
import { CommandError, type DeletedTask } from './api.ts'
import { createDeletedTasks, type DeletedTasksApi } from './deletedTasks.ts'

function deleted(id: string, title = id): DeletedTask {
  return { id, title, status: 'todo', deletedAt: '2026-10-16T09:00:00Z' }
}

/** A fake api over one project's deleted tasks, recording every call. */
function fake(tasks: DeletedTask[], { blocked = new Set<string>() } = {}) {
  const calls: string[] = []
  const api: DeletedTasksApi = {
    listDeletedTasks: async (project) => {
      calls.push(`list ${project}`)
      return [...tasks]
    },
    restoreTask: async (project, id) => {
      calls.push(`restore ${project} ${id}`)
      if (blocked.has(id)) {
        throw new CommandError('parent_not_found', `parent task of \`${id}\` is deleted`)
      }
      tasks = tasks.filter((task) => task.id !== id)
      return null
    },
    purgeDeletedTasks: async (project) => {
      calls.push(`purge ${project}`)
      const count = tasks.length
      tasks = []
      return count
    },
  }
  return { api, calls }
}

test('loads the deleted tasks, newest first as the app sends them', async () => {
  const app = fake([deleted('bbbbbbb'), deleted('aaaaaaa')])
  const model = createDeletedTasks(app.api, 'demo')
  expect(model.getState().tasks).toBeNull()
  await model.load()
  expect(model.getState().tasks?.map((task) => task.id)).toEqual(['bbbbbbb', 'aaaaaaa'])
})

test('Restore brings a task back and reloads the list', async () => {
  const app = fake([deleted('bbbbbbb'), deleted('aaaaaaa')])
  const model = createDeletedTasks(app.api, 'demo')
  await model.load()
  await model.restore('aaaaaaa')
  expect(app.calls).toEqual(['list demo', 'restore demo aaaaaaa', 'list demo'])
  expect(model.getState()).toMatchObject({ tasks: [deleted('bbbbbbb')], error: null, busy: false })
})

test('a refused Restore shows the message and keeps the task', async () => {
  const app = fake([deleted('aaaaaaa')], { blocked: new Set(['aaaaaaa']) })
  const model = createDeletedTasks(app.api, 'demo')
  await model.load()
  await model.restore('aaaaaaa')
  expect(model.getState()).toMatchObject({
    tasks: [deleted('aaaaaaa')],
    error: 'parent task of `aaaaaaa` is deleted',
  })
})

test('Clean up asks first, and nothing is removed until you confirm', async () => {
  const app = fake([deleted('bbbbbbb'), deleted('aaaaaaa')])
  const model = createDeletedTasks(app.api, 'demo')
  await model.load()

  model.askCleanUp()
  expect(model.getState().confirming).toBe(true)
  expect(app.calls).not.toContain('purge demo')

  model.cancelCleanUp()
  expect(model.getState().confirming).toBe(false)
  // Confirming a closed dialog does nothing either.
  await model.confirmCleanUp()
  expect(app.calls).not.toContain('purge demo')
  expect(model.getState().tasks).toHaveLength(2)

  model.askCleanUp()
  await model.confirmCleanUp()
  expect(app.calls.filter((call) => call === 'purge demo')).toHaveLength(1)
  expect(model.getState()).toMatchObject({ tasks: [], confirming: false, busy: false })
})

test('a refused clean up keeps the dialog open with the message', async () => {
  const app = fake([deleted('aaaaaaa')])
  app.api.purgeDeletedTasks = async () => {
    throw new CommandError('internal', 'database is locked')
  }
  const model = createDeletedTasks(app.api, 'demo')
  await model.load()
  model.askCleanUp()
  await model.confirmCleanUp()
  expect(model.getState()).toMatchObject({
    confirming: true,
    cleanUpError: 'database is locked',
    tasks: [deleted('aaaaaaa')],
  })
})

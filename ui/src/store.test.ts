import { expect, test } from 'vitest'
import { CommandError, type ProjectInfo, type TaskSummary, type TasksChanged } from './api.ts'
import { createBoardStore, type BoardDeps } from './store.ts'

function projectInfo(id: string, open = true): ProjectInfo {
  return {
    id,
    name: id,
    repoPath: `/repos/${id}`,
    lastOpenedAt: null,
    open,
    error: open ? null : 'the folder is missing',
    mcpUrl: open ? `http://127.0.0.1:4317/p/${id}/mcp` : null,
  }
}

function task(id: string, title = id): TaskSummary {
  return {
    id,
    title,
    status: 'todo',
    priority: 'medium',
    parent: null,
    labels: [],
    dependsOn: [],
    created: '2026-10-15',
    lease: null,
    blocked: null,
    unmet: [],
  }
}

/** Waits until every pending promise has run. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0))

/** A promise you resolve by hand, to hold a fetch in flight. */
function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((r) => (resolve = r))
  return { promise, resolve }
}

/**
 * A fake desktop app: projects, tasks per project and the event channel.
 * Folders under `/repos/` are in git; others are not.
 */
function fakeApp(projects: ProjectInfo[], tasks: Record<string, TaskSummary[]>) {
  const handlers = new Set<(event: TasksChanged) => void>()
  const fetches: string[] = []
  let hold: { promise: Promise<void> } | null = null
  const deps: BoardDeps = {
    api: {
      listProjects: async () => projects,
      openProject: async (path, allowOutsideGit = false) => {
        const project = projects.find((p) => p.repoPath === path)
        if (project) return { kind: 'project', project }
        if (!path.startsWith('/repos/') && !allowOutsideGit) {
          return { kind: 'outsideGit', repoPath: path }
        }
        const created = projectInfo(path.split('/').pop()!)
        projects = [created, ...projects]
        tasks[created.id] = []
        return { kind: 'project', project: created }
      },
      removeProject: async (id) => {
        projects = projects.filter((p) => p.id !== id)
        return true
      },
      listTasks: async (project) => {
        fetches.push(project)
        if (hold) await hold.promise
        const list = tasks[project]
        if (!list) throw new CommandError('project_not_found', `project \`${project}\` is not open`)
        return list
      },
    },
    onTasksChanged: async (handler) => {
      handlers.add(handler)
      return () => handlers.delete(handler)
    },
  }
  return {
    deps,
    fetches,
    tasks,
    emit: (project: string, changed: string[] = []) => {
      for (const handler of handlers) handler({ project, changed, removed: [] })
    },
    listening: () => handlers.size,
    holdFetches: () => {
      const gate = deferred<void>()
      hold = gate
      return () => {
        hold = null
        gate.resolve()
      }
    },
  }
}

test('start shows the most recent open project and its tasks', async () => {
  const app = fakeApp([projectInfo('gone', false), projectInfo('a'), projectInfo('b')], {
    a: [task('aaaaaaa')],
    b: [],
  })
  const store = createBoardStore(app.deps)
  let renders = 0
  store.subscribe(() => renders++)
  await store.start()
  expect(store.getState()).toMatchObject({
    project: 'a',
    tasks: [task('aaaaaaa')],
    loading: false,
    error: null,
  })
  expect(store.getState().projects.map((p) => p.id)).toEqual(['gone', 'a', 'b'])
  expect(renders).toBeGreaterThan(0)
  // Starting twice, as StrictMode does, listens once.
  await store.start()
  expect(app.listening()).toBe(1)
  store.stop()
  await Promise.resolve()
  expect(app.listening()).toBe(0)
})

test('tasks-changed for the project on screen refetches its tasks', async () => {
  const app = fakeApp([projectInfo('a'), projectInfo('b')], { a: [], b: [] })
  const store = createBoardStore(app.deps)
  await store.start()
  app.fetches.length = 0

  app.tasks.a = [task('kkkkkkk', 'From an agent')]
  app.emit('a', ['kkkkkkk'])
  await settle()
  expect(store.getState().tasks).toEqual([task('kkkkkkk', 'From an agent')])

  app.emit('b')
  await settle()
  expect(app.fetches).toEqual(['a'])
})

test('a burst of events during a fetch costs one more fetch', async () => {
  const app = fakeApp([projectInfo('a')], { a: [] })
  const store = createBoardStore(app.deps)
  await store.start()
  app.fetches.length = 0

  const release = app.holdFetches()
  app.emit('a')
  await settle()
  expect(app.fetches).toEqual(['a'])
  app.emit('a')
  app.emit('a')
  app.emit('a')
  app.tasks.a = [task('zzzzzzz')]
  release()
  await settle()
  expect(app.fetches).toEqual(['a', 'a'])
  expect(store.getState().tasks).toEqual([task('zzzzzzz')])
})

test('opening a folder in a git repo shows it straight away', async () => {
  const app = fakeApp([projectInfo('a')], { a: [task('aaaaaaa')] })
  const store = createBoardStore(app.deps)
  await store.start()

  const opened = await store.openProject('/repos/new')
  expect(opened.kind).toBe('project')
  expect(store.getState()).toMatchObject({ project: 'new', tasks: [], loading: false })
  expect(store.getState().projects[0].id).toBe('new')
})

test('opening a folder outside git changes nothing until allowed', async () => {
  const app = fakeApp([projectInfo('a')], { a: [task('aaaaaaa')] })
  const store = createBoardStore(app.deps)
  await store.start()

  const declined = await store.openProject('/tmp/notes')
  expect(declined).toEqual({ kind: 'outsideGit', repoPath: '/tmp/notes' })
  expect(store.getState().project).toBe('a')
  expect(store.getState().projects.map((p) => p.id)).toEqual(['a'])

  const opened = await store.openProject('/tmp/notes', true)
  expect(opened.kind).toBe('project')
  expect(store.getState()).toMatchObject({ project: 'notes', tasks: [], loading: false })
})

test('switching projects drops a slow fetch for the old one', async () => {
  const app = fakeApp([projectInfo('a'), projectInfo('b')], {
    a: [task('aaaaaaa')],
    b: [task('bbbbbbb')],
  })
  const store = createBoardStore(app.deps)
  await store.start()

  const release = app.holdFetches()
  void store.refetch()
  await settle()
  const switched = store.openProject('/repos/b')
  await settle()
  expect(store.getState()).toMatchObject({ project: 'b', tasks: [], loading: true })
  release()
  await switched
  await settle()
  expect(app.fetches.slice(-2)).toEqual(['a', 'b'])
  expect(store.getState()).toMatchObject({
    project: 'b',
    tasks: [task('bbbbbbb')],
    loading: false,
  })
})

test('removing the project on screen shows the next open one, and errors are kept', async () => {
  const app = fakeApp([projectInfo('a'), projectInfo('b')], { a: [], b: [task('bbbbbbb')] })
  const store = createBoardStore(app.deps)
  await store.start()

  await store.removeProject('a')
  expect(store.getState()).toMatchObject({ project: 'b', tasks: [task('bbbbbbb')] })

  delete app.tasks.b
  await store.refetch()
  expect(store.getState().error).toMatchObject({ code: 'project_not_found' })
  expect(store.getState().tasks).toEqual([task('bbbbbbb')])
})

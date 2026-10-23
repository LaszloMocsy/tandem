import { renderToStaticMarkup } from 'react-dom/server'
import { expect, test } from 'vitest'
import type { ProjectInfo, TaskSummary } from './api.ts'
import App from './App.tsx'
import { createBoardStore } from './store.ts'

const project: ProjectInfo = {
  id: 'tandem',
  name: 'tandem',
  repoPath: '/repos/tandem',
  lastOpenedAt: null,
  open: true,
  error: null,
  mcpUrl: null,
}

function task(id: string, status: TaskSummary['status']): TaskSummary {
  return {
    id,
    title: `Task ${id}`,
    status,
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

function store(projects: ProjectInfo[], tasks: TaskSummary[]) {
  return createBoardStore({
    api: {
      listProjects: async () => projects,
      openProject: async () => ({ kind: 'project', project }),
      removeProject: async () => true,
      listTasks: async () => tasks,
    },
    onTasksChanged: async () => () => {},
  })
}

test('with no project open, shows the welcome screen', () => {
  const html = renderToStaticMarkup(<App store={store([], [])} />)
  expect(html).toContain('Tandem')
  expect(html).toContain('Welcome to Tandem')
  expect(html).toContain('Open folder')
  // Settings are there before any board is.
  expect(html).toContain('title="Settings (Cmd/Ctrl+,)">Settings</button>')
})

test('with a project open, its name is the project switcher', async () => {
  const board = store([project], [])
  await board.start()
  const html = renderToStaticMarkup(<App store={board} />)
  expect(html).toContain('aria-haspopup="menu"')
  expect(html).not.toContain('Welcome to Tandem')
})

test('shows the five columns with their cards', async () => {
  const board = store([project], [task('aaaaaaa', 'todo'), task('bbbbbbb', 'finished')])
  await board.start()
  const html = renderToStaticMarkup(<App store={board} />)
  const columns = [...html.matchAll(/<section class="column" aria-label="([^"]+)"/g)].map(
    (match) => match[1],
  )
  expect(columns).toEqual(['Backlog', 'Todo', 'In progress', 'Review', 'Done'])
  expect(html).toMatch(/aria-label="Todo">.*Task aaaaaaa.*aria-label="In progress"/)
  expect(html).toMatch(/aria-label="Review">.*Task bbbbbbb.*aria-label="Done"/)
})

test('New task is always available on an open board', async () => {
  const board = store([project], [task('aaaaaaa', 'todo')])
  await board.start()
  const html = renderToStaticMarkup(<App store={board} />)
  expect(html).toContain('<button type="button" class="primary new-task-button" title="New task (N)">')
})

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
  readOnly: null,
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
    problem: null,
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

test('shows that no project is open', () => {
  const html = renderToStaticMarkup(<App store={store([], [])} />)
  expect(html).toContain('Tandem')
  expect(html).toContain('No project open.')
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

test('a read-only project says why, such as asking you to update', async () => {
  const reason =
    'the tasks use format version 99, but this app supports only up to 1; update Tandem to edit them'
  const board = store([{ ...project, readOnly: reason }], [task('aaaaaaa', 'todo')])
  await board.start()
  const html = renderToStaticMarkup(<App store={board} />)
  expect(html).toContain(`<p class="read-only-notice" role="status">Read-only: ${reason}</p>`)
  expect(html).toContain('disabled="" title="New task (N)"')
})

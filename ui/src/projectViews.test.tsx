import { renderToStaticMarkup } from 'react-dom/server'
import { expect, test } from 'vitest'
import type { DeleteCheck, ProjectInfo, TaskSummary } from './api.ts'
import { CreateBoardForm, DeleteTaskForm, RemoveProjectForm } from './dialogs.tsx'
import { ProjectSwitcher } from './ProjectSwitcher.tsx'
import { Welcome } from './Welcome.tsx'

const noop = () => {}

const tandem: ProjectInfo = {
  id: 'tandem',
  name: 'tandem',
  repoPath: '/repos/tandem',
  lastOpenedAt: '2026-10-16T10:00:00Z',
  open: true,
  error: null,
  readOnly: null,
  mcpUrl: null,
}
const gone: ProjectInfo = {
  ...tandem,
  id: 'old-api',
  name: 'old-api',
  repoPath: '/repos/old-api',
  open: false,
  error: 'the folder is missing',
}

function welcome(projects: ProjectInfo[]) {
  return renderToStaticMarkup(
    <Welcome
      projects={projects}
      now={Date.parse('2026-10-16T12:00:00Z')}
      onNewBoard={noop}
      onOpenBoard={noop}
      onOpenProject={noop}
      onRemoveProject={noop}
    />,
  )
}

test('the welcome screen offers New board and Open board', () => {
  const html = welcome([])
  expect(html).toContain('Welcome to Tandem')
  expect(html).toContain('New board')
  expect(html).toContain('Open board')
  expect(html).not.toContain('Recent')
})

test('the welcome screen lists recent projects with their problems', () => {
  const html = welcome([tandem, gone])
  expect(html).toContain('aria-label="Recent projects"')
  expect(html).toContain('/repos/tandem')
  expect(html).toMatch(/title="the folder is missing">folder missing</)
  expect(html).toContain('aria-label="Remove old-api from Tandem"')
})

test('the switcher shows the project on screen, closed', () => {
  const html = renderToStaticMarkup(
    <ProjectSwitcher
      projects={[tandem, gone]}
      current={tandem}
      onSwitch={noop}
      onNewBoard={noop}
      onOpenBoard={noop}
      onRemove={noop}
    />,
  )
  expect(html).toContain('aria-expanded="false"')
  expect(html).toContain('title="/repos/tandem"')
  expect(html).not.toContain('role="menu"')
})

function remove(working: { id: string; title: string }[] | null, error: string | null = null) {
  return renderToStaticMarkup(
    <RemoveProjectForm
      project={tandem}
      working={working}
      busy={false}
      error={error}
      onSubmit={noop}
      onRecheck={noop}
      onCancel={noop}
    />,
  )
}

test('removing says the codebase stays and offers two choices', () => {
  const html = remove([])
  expect(html).toContain('Remove “tandem” from Tandem?')
  expect(html).toContain("Tandem won&#x27;t delete <code>/repos/tandem</code>")
  expect(html).toContain('<button type="submit" class="danger">Remove from Tandem</button>')
  expect(html).toContain('>Keep board</button>')
})

test('removing is blocked while an agent works on a task', () => {
  const html = remove([{ id: 'k3f9x2m', title: 'Busy' }])
  expect(html).toContain('An agent is working on this board')
  expect(html).toContain('<code>k3f9x2m</code> Busy')
  expect(html).toContain(
    '<button type="submit" class="danger" disabled="">Remove from Tandem</button>',
  )
  expect(html).toContain('Check again')
})

test('removing waits for the agent check', () => {
  expect(remove(null)).toContain('disabled="">Checking agents…</button>')
})

test('creating a board in a folder without git warns first', () => {
  const html = renderToStaticMarkup(
    <CreateBoardForm
      offer={{ repoPath: '/tmp/notes', gitRepo: false, asked: true }}
      busy={false}
      error={null}
      onSubmit={noop}
      onCancel={noop}
    />,
  )
  expect(html).toContain('Create a board without git?')
  expect(html).toContain('is not in a git repository')
  expect(html).toContain('<code>git init</code>')
  expect(html).toContain('>Create board anyway</button>')
})

test('opening a git repo without a board offers to create one, without a warning', () => {
  const html = renderToStaticMarkup(
    <CreateBoardForm
      offer={{ repoPath: '/repos/web', gitRepo: true, asked: false }}
      busy={false}
      error={null}
      onSubmit={noop}
      onCancel={noop}
    />,
  )
  expect(html).toContain('No board in this folder')
  expect(html).not.toContain('git repository')
  expect(html).toContain('>Create board</button>')
})

function summary(id: string, title: string): TaskSummary {
  return {
    id,
    title,
    status: 'todo',
    priority: 'medium',
    parent: null,
    labels: [],
    dependsOn: [],
    created: '2026-10-16',
    lease: null,
    blocked: null,
    problem: null,
    unmet: [],
  }
}

const dropped = summary('k3f9x2m', 'Dropped idea')
const child = summary('a7rq4ne', 'Child')

function deleteForm(check: DeleteCheck | null, error: string | null = null) {
  return renderToStaticMarkup(
    <DeleteTaskForm
      task={dropped}
      tasks={[dropped, child]}
      check={check}
      busy={false}
      error={error}
      onSubmit={noop}
      onRecheck={noop}
      onCancel={noop}
    />,
  )
}

const clear: DeleteCheck = { children: [], dependents: [], lease: null, committed: false }

test('deleting a task that was never committed warns it is gone for good', () => {
  const html = deleteForm(clear)
  expect(html).toContain('Delete “Dropped idea”?')
  expect(html).toContain('<code>.tandem/k3f9x2m.md</code>')
  expect(html).toContain('never committed')
  expect(html).toContain('<button type="submit" class="danger">Delete task</button>')
  expect(html).toContain('>Keep task</button>')
})

test('deleting a committed task says git keeps it', () => {
  const html = deleteForm({ ...clear, committed: true })
  expect(html).toContain('Git history keeps its last committed version')
  expect(html).not.toContain('never committed')
})

test('deleting waits for the check, and a failed check does not block', () => {
  expect(deleteForm(null)).toContain('disabled="">Checking…</button>')
  const failed = deleteForm(null, 'git broke')
  expect(failed).toContain('git broke')
  expect(failed).toContain('<button type="submit" class="danger">Delete task</button>')
})

test('deleting is blocked by an agent at work and by linked tasks', () => {
  const html = deleteForm({
    children: ['a7rq4ne'],
    dependents: ['a7rq4ne'],
    lease: {
      agentId: 'claude-code-7f2a',
      claimedAt: '2026-10-16T09:00:00Z',
      expiresAt: '2026-10-16T09:15:00Z',
      live: true,
    },
    committed: true,
  })
  expect(html).toContain('<code>claude-code-7f2a</code>) is working on this task')
  expect(html).toContain('It is the parent of:')
  expect(html).toContain('These tasks depend on it:')
  expect(html).toContain('<code>a7rq4ne</code> Child')
  expect(html).toContain('Unlink them first')
  expect(html).toContain('<button type="submit" class="danger" disabled="">Delete task</button>')
})

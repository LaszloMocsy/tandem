import { renderToStaticMarkup } from 'react-dom/server'
import { expect, test } from 'vitest'
import type { DeleteCheck, ProjectInfo, TaskSummary } from './api.ts'
import { DeleteTaskForm, OutsideGitForm, RemoveProjectForm } from './dialogs.tsx'
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
  mcpUrl: null,
  dbPath: '/data/db/projects/tandem.db',
  backupsPath: '/data/backups/tandem',
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
      onOpenFolder={noop}
      onOpenProject={noop}
      onRemoveProject={noop}
    />,
  )
}

test('the welcome screen offers Open folder', () => {
  const html = welcome([])
  expect(html).toContain('Welcome to Tandem')
  expect(html).toContain('Open folder')
  expect(html).not.toContain('New board')
  expect(html).not.toContain('.tandem')
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
      onOpenFolder={noop}
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

test('removing says the tasks go but a last backup stays, the codebase stays, and offers two choices', () => {
  const html = remove([])
  expect(html).toContain('Remove “tandem” from Tandem?')
  expect(html).toContain("Tandem won&#x27;t touch <code>/repos/tandem</code>")
  expect(html).toContain('deletes this board&#x27;s database from this machine')
  expect(html).toContain('every task, its changelog and working log')
  expect(html).toContain('keeps one last backup and deletes the others')
  expect(html).toContain('restore the last backup from Settings')
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

test('opening a folder outside git warns first', () => {
  const html = renderToStaticMarkup(
    <OutsideGitForm repoPath="/tmp/notes" busy={false} error={null} onSubmit={noop} onCancel={noop} />,
  )
  expect(html).toContain('Open a folder outside git?')
  expect(html).toContain('<code>/tmp/notes</code> is not in a git repository')
  expect(html).toContain('<code>git init</code>')
  expect(html).toContain('<button type="submit" class="primary">Open anyway</button>')
  expect(html).not.toContain('.tandem')
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

const clear: DeleteCheck = { children: [], dependents: [], lease: null }

test('deleting a task says it can be restored until deleted tasks are cleaned up', () => {
  const html = deleteForm(clear)
  expect(html).toContain('Delete “Dropped idea”?')
  expect(html).toContain('moves to Deleted tasks')
  expect(html).toContain('restore it from Settings → Project until deleted tasks are cleaned up')
  expect(html).not.toMatch(/git|commit|\.tandem/i)
  expect(html).toContain('<button type="submit" class="danger">Delete task</button>')
  expect(html).toContain('>Keep task</button>')
})

test('deleting waits for the check, and a failed check does not block', () => {
  expect(deleteForm(null)).toContain('disabled="">Checking…</button>')
  const failed = deleteForm(null, 'database is locked')
  expect(failed).toContain('database is locked')
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
  })
  expect(html).toContain('<code>claude-code-7f2a</code>) is working on this task')
  expect(html).toContain('It is the parent of:')
  expect(html).toContain('These tasks depend on it:')
  expect(html).toContain('<code>a7rq4ne</code> Child')
  expect(html).toContain('Unlink them first')
  expect(html).toContain('<button type="submit" class="danger" disabled="">Delete task</button>')
})

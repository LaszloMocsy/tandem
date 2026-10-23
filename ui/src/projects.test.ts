import { expect, test } from 'vitest'
import type { AgentInfo, Opened, ProjectInfo } from './api.ts'
import {
  openedAgo,
  openFolder,
  openOutsideGit,
  problemLabel,
  workingTasks,
  type FlowDeps,
} from './projects.ts'

function projectInfo(name: string): ProjectInfo {
  return {
    id: name,
    name,
    repoPath: `/repos/${name}`,
    lastOpenedAt: null,
    open: true,
    error: null,
    mcpUrl: null,
  }
}

/**
 * A fake picker and store over folders: those in `git` are in a git repo,
 * and `projects` collects the ones that became projects. Records every
 * `openProject` call.
 */
function fake(picked: string | null, folders: { git?: string[] } = {}) {
  const git = new Set(folders.git ?? [])
  const projects = new Set<string>()
  const calls: [string, boolean][] = []
  const deps: FlowDeps = {
    chooseFolder: async () => picked,
    openProject: async (path, allowOutsideGit = false): Promise<Opened> => {
      calls.push([path, allowOutsideGit])
      if (!git.has(path) && !allowOutsideGit) return { kind: 'outsideGit', repoPath: path }
      projects.add(path)
      return { kind: 'project', project: projectInfo(path.split('/').pop()!) }
    },
  }
  return { deps, calls, projects }
}

test('cancelling the folder picker does nothing', async () => {
  const app = fake(null)
  expect(await openFolder(app.deps)).toEqual({ kind: 'cancelled' })
  expect(app.calls).toEqual([])
})

test('Open folder in a git repo opens its board straight away', async () => {
  const app = fake('/repos/api', { git: ['/repos/api'] })
  const outcome = await openFolder(app.deps)
  expect(outcome).toMatchObject({ kind: 'opened', project: { id: 'api' } })
  expect(app.calls).toEqual([['/repos/api', false]])
})

test('Open folder outside git asks before adding anything', async () => {
  const app = fake('/tmp/notes')
  const outcome = await openFolder(app.deps)
  expect(outcome).toEqual({ kind: 'outsideGit', repoPath: '/tmp/notes' })
  expect(app.projects.has('/tmp/notes')).toBe(false)

  if (outcome.kind !== 'outsideGit') throw new Error('expected a confirmation')
  expect(await openOutsideGit(app.deps, outcome.repoPath)).toMatchObject({
    kind: 'opened',
    project: { id: 'notes' },
  })
  expect(app.projects.has('/tmp/notes')).toBe(true)
  expect(app.calls).toEqual([
    ['/tmp/notes', false],
    ['/tmp/notes', true],
  ])
})

test('switching to a listed project skips the picker and the confirmation', async () => {
  const app = fake(null)
  expect(await openFolder(app.deps, '/tmp/notes', true)).toMatchObject({ kind: 'opened' })
  expect(app.calls).toEqual([['/tmp/notes', true]])
})

test('only live leases count as agents working', () => {
  const agents: AgentInfo[] = [
    {
      agentId: 'claude-code-1',
      clientName: 'claude-code',
      lastSeenAt: '2026-10-16T09:00:00.000Z',
      tasks: [
        { id: 'k3f9x2m', title: 'Busy', live: true },
        { id: 'b2b2b2b', title: 'Stale', live: false },
      ],
    },
    {
      agentId: 'codex-2',
      clientName: null,
      lastSeenAt: '2026-10-16T09:00:00.000Z',
      tasks: [{ id: 'a7rq4ne', title: 'Also busy', live: true }],
    },
  ]
  expect(workingTasks(agents)).toEqual([
    { id: 'a7rq4ne', title: 'Also busy' },
    { id: 'k3f9x2m', title: 'Busy' },
  ])
  expect(workingTasks([])).toEqual([])
})

test('problems and times read short', () => {
  expect(problemLabel('the folder is missing')).toBe('folder missing')
  expect(problemLabel('the project path is not a folder')).toBe('not a folder')
  expect(problemLabel('the project is not open')).toBe('not open')
  expect(problemLabel('permission denied')).toBe("can't open")

  const now = Date.parse('2026-10-16T12:00:00Z')
  expect(openedAgo(null, now)).toBe('never opened')
  expect(openedAgo('2026-10-16T11:59:40Z', now)).toBe('just now')
  expect(openedAgo('2026-10-16T10:00:00Z', now)).toMatch(/2 hours ago/)
  expect(openedAgo('2026-10-13T12:00:00Z', now)).toMatch(/3 days ago/)
})

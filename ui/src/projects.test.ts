import { expect, test } from 'vitest'
import type { AgentInfo, Opened, ProjectInfo } from './api.ts'
import {
  createBoard,
  newBoard,
  openBoard,
  openedAgo,
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
    readOnly: null,
    mcpUrl: null,
  }
}

/**
 * A fake picker and store over folders: `boards` have `.tandem/`, `git`
 * are git repos. Records every `openProject` call.
 */
function fake(picked: string | null, folders: { boards?: string[]; git?: string[] } = {}) {
  const boards = new Set(folders.boards ?? [])
  const git = new Set(folders.git ?? [])
  const calls: [string, boolean][] = []
  const deps: FlowDeps = {
    chooseFolder: async () => picked,
    openProject: async (path, createFolder = false): Promise<Opened> => {
      calls.push([path, createFolder])
      if (createFolder) boards.add(path)
      if (!boards.has(path)) return { kind: 'noTaskFolder', repoPath: path, gitRepo: git.has(path) }
      return { kind: 'project', project: projectInfo(path.split('/').pop()!) }
    },
  }
  return { deps, calls, boards }
}

test('cancelling the folder picker does nothing', async () => {
  const app = fake(null)
  expect(await newBoard(app.deps)).toEqual({ kind: 'cancelled' })
  expect(await openBoard(app.deps)).toEqual({ kind: 'cancelled' })
  expect(app.calls).toEqual([])
})

test('New board in a git repo creates .tandem/ straight away', async () => {
  const app = fake('/repos/api', { git: ['/repos/api'] })
  const outcome = await newBoard(app.deps)
  expect(outcome).toMatchObject({ kind: 'opened', existing: false, project: { id: 'api' } })
  expect(app.calls).toEqual([
    ['/repos/api', false],
    ['/repos/api', true],
  ])
})

test('New board in a folder without git warns before creating anything', async () => {
  const app = fake('/tmp/notes')
  const outcome = await newBoard(app.deps)
  expect(outcome).toEqual({
    kind: 'offer',
    offer: { repoPath: '/tmp/notes', gitRepo: false, asked: true },
  })
  expect(app.boards.has('/tmp/notes')).toBe(false)

  if (outcome.kind !== 'offer') throw new Error('expected an offer')
  expect(await createBoard(app.deps, outcome.offer)).toMatchObject({ kind: 'opened' })
  expect(app.boards.has('/tmp/notes')).toBe(true)
})

test('New board on a folder that has a board opens it and says so', async () => {
  const app = fake('/repos/api', { boards: ['/repos/api'], git: ['/repos/api'] })
  expect(await newBoard(app.deps)).toMatchObject({ kind: 'opened', existing: true })
  expect(app.calls).toEqual([['/repos/api', false]])
})

test('Open board offers to create .tandem/ when the folder has none', async () => {
  const app = fake('/repos/web', { git: ['/repos/web'] })
  expect(await openBoard(app.deps)).toEqual({
    kind: 'offer',
    offer: { repoPath: '/repos/web', gitRepo: true, asked: false },
  })
  expect(app.boards.has('/repos/web')).toBe(false)
})

test('Open board with a path skips the picker', async () => {
  const app = fake(null, { boards: ['/repos/api'] })
  expect(await openBoard(app.deps, '/repos/api')).toMatchObject({
    kind: 'opened',
    existing: false,
  })
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
  expect(problemLabel('the folder has no .tandem folder')).toBe('no board')
  expect(problemLabel('permission denied')).toBe("can't open")

  const now = Date.parse('2026-10-16T12:00:00Z')
  expect(openedAgo(null, now)).toBe('never opened')
  expect(openedAgo('2026-10-16T11:59:40Z', now)).toBe('just now')
  expect(openedAgo('2026-10-16T10:00:00Z', now)).toMatch(/2 hours ago/)
  expect(openedAgo('2026-10-13T12:00:00Z', now)).toMatch(/3 days ago/)
})

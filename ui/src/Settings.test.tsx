import { renderToStaticMarkup } from 'react-dom/server'
import { expect, test } from 'vitest'
import type { AppSettings, BackupInfo, DeletedTask, ProjectInfo } from './api.ts'
import { APP_VERSION, type BuildEnv } from './buildEnv.ts'
import { CleanUpDeletedForm, RestoreBackupForm } from './dialogs.tsx'
import {
  AppSettingsView,
  BackupsView,
  DeletedTasksView,
  ProjectSettingsView,
  Settings,
  type SettingsApi,
  type UpdateStatus,
  UpdatesView,
} from './Settings.tsx'
import { isSettingsShortcut } from './shortcuts.ts'

const noop = () => {}

const running: AppSettings = {
  mcpPort: 4317,
  defaultMcpPort: 4317,
  mcpUrl: 'http://127.0.0.1:4317',
  mcpError: null,
  theme: 'system',
  appDbPath: '/data/db/app.db',
  updateChannel: 'stable',
  checkUpdates: true,
}

const project: ProjectInfo = {
  id: 'tandem-2',
  name: 'Tandem board',
  repoPath: '/repos/tandem',
  lastOpenedAt: null,
  open: true,
  error: null,
  mcpUrl: 'http://127.0.0.1:4317/p/tandem-2/mcp',
  dbPath: '/data/db/projects/tandem-2.db',
  backupsPath: '/data/backups/tandem-2',
}

const never = () => new Promise<never>(() => {})
const api: SettingsApi = {
  getAppSettings: never,
  setMcpPort: never,
  setTheme: never,
  setUpdateChannel: never,
  setCheckUpdates: never,
  checkForUpdate: never,
  setProjectName: never,
  listDeletedTasks: never,
  restoreTask: never,
  purgeDeletedTasks: never,
  listBackups: never,
  backUpProject: never,
  restoreBackup: never,
  exportTasks: never,
  openLink: never,
  showInFolder: never,
}

function appView(settings: AppSettings, portText = String(settings.mcpPort), portError = null) {
  return renderToStaticMarkup(
    <AppSettingsView
      settings={settings}
      portText={portText}
      portError={portError}
      themeError={null}
      busy={false}
      onPortText={noop}
      onApplyPort={noop}
      onTheme={noop}
      linkError={null}
      onOpenLink={noop}
      showError={null}
      onShow={noop}
    />,
  )
}

function projectView(info: ProjectInfo, name = info.name) {
  return renderToStaticMarkup(
    <ProjectSettingsView
      project={info}
      name={name}
      nameError={null}
      saved={false}
      busy={false}
      onName={noop}
      onSaveName={noop}
      onRemove={noop}
      showError={null}
      onShow={noop}
    />,
  )
}

test('the App tab shows where the server runs and the theme', () => {
  const html = appView({ ...running, theme: 'dark' })
  expect(html).toContain('Running at <code>http://127.0.0.1:4317</code>')
  expect(html).toContain('value="4317"')
  // The port it already runs on can't be applied again.
  expect(html).toMatch(/<button type="submit" class="primary" disabled="">Apply/)
  expect(html).not.toContain('Use the default')
  expect(html).toContain('changes every board&#x27;s MCP URL')
  expect(html).toContain('<input type="radio" name="theme" checked="" value="dark"/>')
  expect(html).toContain('<input type="radio" name="theme" value="system"/>')
})

test('the App tab says why the server is not running, and offers the default port', () => {
  const html = appView({
    ...running,
    mcpPort: 4400,
    mcpUrl: null,
    mcpError: 'port 4400 is in use, maybe by another copy of Tandem; pick another',
  })
  expect(html).toContain('Not running: port 4400 is in use')
  // Retrying the same port is allowed while nothing runs.
  expect(html).toMatch(/<button type="submit" class="primary">Apply/)
  expect(html).toContain('Use the default, 4317')
})

test('a port that is not a number is refused before it is sent', () => {
  const html = appView(running, '70000')
  expect(html).toContain('The port must be between 1 and 65535.')
  expect(html).toMatch(/aria-invalid="true"/)
  expect(html).toMatch(/<button type="submit" class="primary" disabled="">Apply/)
})

test('the App tab shows the running version, with the build for non-production builds', () => {
  const view = (env: BuildEnv, version: string) =>
    renderToStaticMarkup(
      <AppSettingsView
        settings={running}
        portText="4317"
        portError={null}
        themeError={null}
        busy={false}
        onPortText={noop}
        onApplyPort={noop}
        onTheme={noop}
        linkError={null}
        onOpenLink={noop}
        showError={null}
        onShow={noop}
        env={env}
        version={version}
      />,
    )
  expect(view('production', '0.2.0')).toContain('<code>Tandem v0.2.0</code>')
  expect(view('beta', '0.2.0-beta.1')).toContain('<code>Tandem v0.2.0-beta.1 (BETA build)</code>')
  expect(view('alpha', '0.2.0-alpha.3')).toContain(
    '<code>Tandem v0.2.0-alpha.3 (ALPHA build)</code>',
  )
  expect(view('dev', '0.1.0')).toContain('<code>Tandem v0.1.0 (DEV build)</code>')
  expect(view('production', '0.2.0')).toContain('aria-label="Copy version"')
  // Release notes link to this version's release; a dev build's version may not be out yet.
  expect(view('production', '0.2.0')).toContain('>Release notes</button>')
  expect(view('dev', '0.1.0')).toContain('>Releases</button>')
  expect(view('dev', '0.1.0')).toContain('>Repository</button>')
  // Without overrides it describes the running build, which tests run as dev.
  expect(appView(running)).toContain(`<code>Tandem v${APP_VERSION} (DEV build)</code>`)
})

test('the Project tab shows the name, the local values and the actions', () => {
  const html = projectView(project)
  expect(html).toContain('value="Tandem board"')
  expect(html).toContain('placeholder="tandem"')
  expect(html).toContain('Stored on this machine. Leave it empty to use the folder name')
  expect(html).not.toMatch(/\.tandem|commit/)
  expect(html).toMatch(/disabled="">Save<\/button>/)
  expect(html).toContain('<code>tandem-2</code><button type="button" class="link" aria-label="Copy project id">Copy</button>')
  expect(html).toContain(
    '<code>http://127.0.0.1:4317/p/tandem-2/mcp</code><button type="button" class="link" aria-label="Copy MCP URL">Copy</button>',
  )
  expect(html).not.toContain('Reset local state')
  expect(html).toContain('Deletes this board and its tasks from this machine, keeping one last backup')
  expect(html).toContain('Remove…')
})

test('the App tab shows where the app database is, to open in the file manager', () => {
  const html = renderToStaticMarkup(
    <AppSettingsView
      settings={running}
      portText="4317"
      portError={null}
      themeError={null}
      busy={false}
      onPortText={noop}
      onApplyPort={noop}
      onTheme={noop}
      linkError={null}
      onOpenLink={noop}
      showError="the file is missing"
      onShow={noop}
      manager="Finder"
    />,
  )
  expect(html).toContain(
    '<dt>App database</dt><dd><code>/data/db/app.db</code><button type="button" class="link" aria-label="Copy the app database path">Copy</button><button type="button" class="link">Show in Finder</button></dd>',
  )
  expect(html).toContain('the file is missing')
  // Data comes before About.
  expect(html.indexOf('aria-label="Data"')).toBeLessThan(html.indexOf('aria-label="About"'))
})

test('the Project tab shows where its database and backups are', () => {
  const html = renderToStaticMarkup(
    <ProjectSettingsView
      project={project}
      name={project.name}
      nameError={null}
      saved={false}
      busy={false}
      onName={noop}
      onSaveName={noop}
      onRemove={noop}
      showError={null}
      onShow={noop}
      manager="Explorer"
    />,
  )
  expect(html).toContain(
    '<dt>Database</dt><dd><code>/data/db/projects/tandem-2.db</code><button type="button" class="link" aria-label="Copy the database path">Copy</button><button type="button" class="link">Show in Explorer</button></dd>',
  )
  // The backups folder itself opens.
  expect(html).toContain(
    '<dt>Backups</dt><dd><code>/data/backups/tandem-2</code><button type="button" class="link" aria-label="Copy the backups path">Copy</button><button type="button" class="link">Open in Explorer</button></dd>',
  )
  expect(html).toContain('never in the repo')
})

test('a new name can be saved, and an empty one goes back to the folder name', () => {
  expect(projectView(project, 'Renamed')).toMatch(/<button type="submit" class="primary">Save/)
  expect(projectView(project, '  ')).toMatch(/<button type="submit" class="primary">Save/)
  // Without a name of its own, clearing it changes nothing.
  const unnamed = { ...project, name: 'tandem' }
  expect(projectView(unnamed, '')).toMatch(/disabled="">Save/)
  expect(projectView(project, 'two\nlines')).toContain("can&#x27;t contain line breaks")
})

test('without the MCP server, the Project tab points to the App tab', () => {
  const html = projectView({ ...project, mcpUrl: null })
  expect(html).toContain('The MCP server isn&#x27;t running; see the App tab.')
})

test('without an open board, only the App tab can be chosen', () => {
  const html = renderToStaticMarkup(
    <Settings
      tab="project"
      project={undefined}
      tasks={[]}
      api={api}
      onTab={noop}
      onClose={noop}
      onProjectsChanged={noop}
      onRemove={noop}
      onUpdate={noop}
    />,
  )
  expect(html).toMatch(/role="tab" class="tab" aria-selected="true"[^>]*>App</)
  expect(html).toMatch(/aria-selected="false"[^>]*disabled=""[^>]*>Project</)
  expect(html).toContain('Loading…')
})

const deletedTasks: DeletedTask[] = [
  { id: 'k3f9x2m', title: 'Dropped idea', status: 'todo', deletedAt: '2026-10-16T09:00:00Z' },
  { id: 'a7rq4ne', title: 'Old spike', status: 'done', deletedAt: '2026-10-15T09:00:00Z' },
]

function deletedView(
  tasks: DeletedTask[] | null,
  { error = null as string | null, busy = false } = {},
) {
  return renderToStaticMarkup(
    <DeletedTasksView
      tasks={tasks}
      loadError={null}
      busy={busy}
      error={error}
      onRestore={noop}
      onCleanUp={noop}
    />,
  )
}

test('the Deleted tasks section lists each deleted task with Restore', () => {
  const html = deletedView(deletedTasks)
  expect(html).toContain('<h3>Deleted tasks</h3>')
  expect(html).toMatch(/<code>k3f9x2m<\/code><span class="deleted-title">Dropped idea<\/span><span class="muted">deleted [^<]+<\/span>/)
  expect(html).toContain('<code>a7rq4ne</code><span class="deleted-title">Old spike</span>')
  expect(html).toContain('aria-label="Restore k3f9x2m">Restore</button>')
  expect(html).toContain('aria-label="Restore a7rq4ne">Restore</button>')
  expect(html).toMatch(/<button type="button" class="delete-task">Clean up…<\/button>/)
})

test('a refused restore says why', () => {
  const html = deletedView(deletedTasks, {
    error: 'parent task `a7rq4ne` was not found',
  })
  expect(html).toContain('<p class="field-error" role="alert">parent task `a7rq4ne` was not found</p>')
})

test('without deleted tasks, the section says so and Clean up is disabled', () => {
  const html = deletedView([])
  expect(html).toContain('No deleted tasks.')
  expect(html).not.toContain('>Restore</button>')
  expect(html).toMatch(/<button type="button" class="delete-task" disabled="">Clean up…<\/button>/)
  expect(deletedView(null)).toContain('Loading…')
  expect(deletedView(null)).toMatch(/disabled="">Clean up…/)
})

const backups: BackupInfo[] = [
  { name: '20261023T090000.000Z', createdAt: '2026-10-23T09:00:00.000Z', size: 40960, tasks: 12 },
  { name: '20261022T090000.000Z', createdAt: '2026-10-22T09:00:00.000Z', size: 900, tasks: null },
]

function backupsView(list: BackupInfo[] | null, { notice = null as string | null } = {}) {
  return renderToStaticMarkup(
    <BackupsView
      backups={list}
      loadError={null}
      busy={false}
      notice={notice}
      error={null}
      onBackUp={noop}
      onRestore={noop}
      onExport={noop}
    />,
  )
}

test('the Backups section lists each backup with Restore, Back up now and Export', () => {
  const html = backupsView(backups)
  expect(html).toContain('<h3>Backups</h3>')
  expect(html).toContain('12 tasks · 40 KB')
  expect(html).toMatch(/aria-label="Restore the backup from [^"]+">Restore…<\/button>/)
  // An unreadable backup can't be restored.
  expect(html).toContain('unreadable · 900 B')
  expect(html).toMatch(/disabled="">Restore…<\/button>/)
  expect(html).toContain('>Back up now</button>')
  expect(html).toContain('>JSON…</button>')
  expect(html).toContain('>Markdown…</button>')
  expect(html).toContain('every hour while it changes')
  expect(html).not.toContain('.tandem')
})

test('without backups, the section says so', () => {
  expect(backupsView([])).toContain('No backups yet.')
  expect(backupsView(null)).toContain('Loading…')
  expect(backupsView([], { notice: 'Exported to /tmp/x.json.' })).toContain(
    '<p class="muted" role="status">Exported to /tmp/x.json.</p>',
  )
})

test('the restore dialog says what is replaced and how to undo it', () => {
  const html = renderToStaticMarkup(
    <RestoreBackupForm
      when="Oct 23, 09:00"
      tasks="12 tasks"
      busy={false}
      error={null}
      onSubmit={noop}
      onCancel={noop}
    />,
  )
  expect(html).toContain('Restore the backup from Oct 23, 09:00?')
  expect(html).toContain('replaced with the backup&#x27;s 12 tasks')
  expect(html).toContain('backed up first')
  expect(html).toContain('<button type="submit" class="danger">Restore backup</button>')
  expect(html).toContain('>Keep current tasks</button>')
})

test('the Project tab places Backups and Deleted tasks before Remove', () => {
  const html = renderToStaticMarkup(
    <ProjectSettingsView
      project={project}
      name={project.name}
      nameError={null}
      saved={false}
      busy={false}
      onName={noop}
      onSaveName={noop}
      onRemove={noop}
      showError={null}
      onShow={noop}
      backups={<section aria-label="Backups" />}
      deletedTasks={<section aria-label="Deleted tasks" />}
    />,
  )
  expect(html.indexOf('aria-label="Backups"')).toBeGreaterThan(-1)
  expect(html.indexOf('aria-label="Backups"')).toBeLessThan(
    html.indexOf('aria-label="Deleted tasks"'),
  )
  expect(html.indexOf('aria-label="Deleted tasks"')).toBeGreaterThan(-1)
  expect(html.indexOf('aria-label="Deleted tasks"')).toBeLessThan(html.indexOf('Remove…'))
})

test('the clean up dialog says it is permanent and the tasks can no longer be restored', () => {
  const render = (count: number) =>
    renderToStaticMarkup(
      <CleanUpDeletedForm count={count} busy={false} error={null} onSubmit={noop} onCancel={noop} />,
    )
  const html = render(2)
  expect(html).toContain('Clean up 2 deleted tasks?')
  expect(html).toContain('permanently deletes the 2 deleted tasks and their history')
  expect(html).toContain('This can&#x27;t be undone.')
  expect(html).toContain('resets the undo history')
  expect(html).toContain('none of them can be restored')
  expect(html).toContain('<button type="submit" class="danger">Clean up for good</button>')
  expect(html).toContain('>Keep them</button>')
  expect(render(1)).toContain('Clean up 1 deleted task?')
})

test('Cmd or Ctrl+comma opens Settings', () => {
  const key = (key: string, mods: Partial<Record<'metaKey' | 'ctrlKey' | 'altKey', boolean>>) => ({
    key,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    ...mods,
  })
  expect(isSettingsShortcut(key(',', { metaKey: true }))).toBe(true)
  expect(isSettingsShortcut(key(',', { ctrlKey: true }))).toBe(true)
  expect(isSettingsShortcut(key(',', {}))).toBe(false)
  expect(isSettingsShortcut(key(',', { metaKey: true, altKey: true }))).toBe(false)
})

function updatesView(settings: AppSettings, status: UpdateStatus = { kind: 'idle' }) {
  return renderToStaticMarkup(
    <UpdatesView
      settings={settings}
      status={status}
      onChannel={noop}
      onCheckOnStart={noop}
      onCheckNow={noop}
      onOpenLink={noop}
    />,
  )
}

test('Updates shows the channel, the check on start and Check now', () => {
  const html = updatesView({ ...running, updateChannel: 'beta', checkUpdates: false })
  expect(html).toContain('<input type="radio" name="update-channel" checked="" value="beta"/>')
  expect(html).toContain('<input type="radio" name="update-channel" value="stable"/>')
  expect(html).toContain('<input type="checkbox"/>Check for updates when Tandem starts')
  expect(html).toContain('>Check now</button>')
})

test('Check now says how the check went', () => {
  expect(updatesView(running, { kind: 'checking' })).toContain('disabled="">Checking…</button>')
  expect(updatesView(running, { kind: 'upToDate' })).toContain('Tandem is up to date.')
  expect(updatesView(running, { kind: 'error', message: 'offline' })).toContain(
    '<p class="field-error" role="alert">offline</p>',
  )
})

test('dev and alpha builds have no update settings', () => {
  const html = updatesView({ ...running, updateChannel: null })
  expect(html).toContain('don&#x27;t update themselves')
  expect(html).not.toContain('update-channel')
  expect(html).not.toContain('Check now')
})

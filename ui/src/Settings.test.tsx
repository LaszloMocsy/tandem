import { renderToStaticMarkup } from 'react-dom/server'
import { expect, test } from 'vitest'
import type { AppSettings, ProjectInfo } from './api.ts'
import { APP_VERSION, type BuildEnv } from './buildEnv.ts'
import { ResetLocalStateForm } from './dialogs.tsx'
import { AppSettingsView, ProjectSettingsView, Settings, type SettingsApi } from './Settings.tsx'
import { isSettingsShortcut } from './shortcuts.ts'

const noop = () => {}

const running: AppSettings = {
  mcpPort: 4317,
  defaultMcpPort: 4317,
  mcpUrl: 'http://127.0.0.1:4317',
  mcpError: null,
  theme: 'system',
}

const project: ProjectInfo = {
  id: 'tandem-2',
  name: 'Tandem board',
  repoPath: '/repos/tandem',
  lastOpenedAt: null,
  open: true,
  error: null,
  readOnly: null,
  mcpUrl: 'http://127.0.0.1:4317/p/tandem-2/mcp',
}

const never = () => new Promise<never>(() => {})
const api: SettingsApi = {
  getAppSettings: never,
  setMcpPort: never,
  setTheme: never,
  setProjectName: never,
  resetProject: never,
  listAgents: never,
  openLink: never,
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
      onReset={noop}
      onRemove={noop}
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
  expect(html).toContain('Saved in <code>.tandem/config.toml</code>')
  expect(html).toMatch(/disabled="">Save<\/button>/)
  expect(html).toContain('<code>tandem-2</code><button type="button" class="link" aria-label="Copy project id">Copy</button>')
  expect(html).toContain(
    '<code>http://127.0.0.1:4317/p/tandem-2/mcp</code><button type="button" class="link" aria-label="Copy MCP URL">Copy</button>',
  )
  expect(html).toContain('Reset local state…')
  expect(html).toContain('Remove…')
})

test('a new name can be saved, and an empty one goes back to the folder name', () => {
  expect(projectView(project, 'Renamed')).toMatch(/<button type="submit" class="primary">Save/)
  expect(projectView(project, '  ')).toMatch(/<button type="submit" class="primary">Save/)
  // Without a name in config.toml, clearing it changes nothing.
  const unnamed = { ...project, name: 'tandem' }
  expect(projectView(unnamed, '')).toMatch(/disabled="">Save/)
  expect(projectView(project, 'two\nlines')).toContain("can&#x27;t contain line breaks")
})

test('a read-only project shows why its name cannot change', () => {
  const html = projectView({ ...project, readOnly: 'the tasks use format version 99', mcpUrl: null })
  expect(html).toMatch(/<input [^>]*aria-label="Board name"[^>]*disabled=""/)
  expect(html).toContain('Read-only: the tasks use format version 99')
  expect(html).toContain('The MCP server isn&#x27;t running; see the App tab.')
})

test('without an open board, only the App tab can be chosen', () => {
  const html = renderToStaticMarkup(
    <Settings
      tab="project"
      project={undefined}
      api={api}
      onTab={noop}
      onClose={noop}
      onProjectsChanged={noop}
      onReset={noop}
      onRemove={noop}
    />,
  )
  expect(html).toMatch(/role="tab" class="tab" aria-selected="true"[^>]*>App</)
  expect(html).toMatch(/aria-selected="false"[^>]*disabled=""[^>]*>Project</)
  expect(html).toContain('Loading…')
})

test('the reset dialog warns about the tasks agents are working on', () => {
  const render = (working: { id: string; title: string }[] | null) =>
    renderToStaticMarkup(
      <ResetLocalStateForm
        project={project}
        working={working}
        busy={false}
        error={null}
        onSubmit={noop}
        onCancel={noop}
      />,
    )
  const html = render([{ id: 'k3f9x2m', title: 'Fix token refresh race' }])
  expect(html).toContain('Reset local state of “Tandem board”?')
  expect(html).toContain('Leases, blocked flags, review feedback and working logs are lost.')
  expect(html).toContain('Agents are working on this task.')
  expect(html).toContain('<code>k3f9x2m</code> Fix token refresh race')
  expect(html).toMatch(/<button type="submit" class="danger">Reset local state/)
  expect(render([])).not.toContain('Agents are working')
  expect(render(null)).toMatch(/disabled="">Checking agents…/)
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

import { useEffect, useId, useState, type KeyboardEvent } from 'react'
import type { Api, AppSettings, Link, ProjectInfo, Theme } from './api.ts'
import { APP_VERSION, BUILD_ENV, type BuildEnv, versionText } from './buildEnv.ts'
import { Dialog, ResetLocalStateForm } from './dialogs.tsx'
import { errorMessage } from './edit.ts'
import { CopyButton } from './panels.tsx'
import { workingTasks, type WorkingTask } from './projects.ts'
import { folderName, nameProblem, parsePort } from './settingsInput.ts'
import { applyTheme } from './theme.ts'

export type SettingsTab = 'app' | 'project'

/** The commands the Settings page uses. */
export type SettingsApi = Pick<
  Api,
  | 'getAppSettings'
  | 'setMcpPort'
  | 'setTheme'
  | 'setProjectName'
  | 'resetProject'
  | 'listAgents'
  | 'openLink'
>

const THEMES: { theme: Theme; label: string }[] = [
  { theme: 'system', label: 'System' },
  { theme: 'light', label: 'Light' },
  { theme: 'dark', label: 'Dark' },
]

/**
 * The Settings page, in the board's place: the App tab for the whole
 * install, and the Project tab for the board on screen. The Project tab
 * needs an open project; without one, the App tab shows.
 */
export function Settings({
  tab,
  project,
  api,
  onTab,
  onClose,
  onProjectsChanged,
  onReset,
  onRemove,
}: {
  tab: SettingsTab
  project: ProjectInfo | undefined
  api: SettingsApi
  onTab: (tab: SettingsTab) => void
  onClose: () => void
  /** A project's name or MCP URL changed, so the list needs reloading. */
  onProjectsChanged: () => void
  /** The project's local state was reset. */
  onReset: (project: ProjectInfo) => void
  onRemove: (project: ProjectInfo) => void
}) {
  const id = useId()
  const projectTab = project?.open ? project : undefined
  const shown: SettingsTab = projectTab ? tab : 'app'
  const tabs: { tab: SettingsTab; label: string; disabled: boolean }[] = [
    { tab: 'app', label: 'App', disabled: false },
    { tab: 'project', label: 'Project', disabled: projectTab === undefined },
  ]

  function onTabKeyDown(event: KeyboardEvent) {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
    event.preventDefault()
    const next = shown === 'app' ? 'project' : 'app'
    if (next === 'project' && !projectTab) return
    onTab(next)
    document.getElementById(`${id}-${next}`)?.focus()
  }

  return (
    <main className="settings" aria-label="Settings">
      <div className="settings-inner">
        <div className="settings-head">
          <h2>Settings</h2>
          <button type="button" className="close" aria-label="Close settings" onClick={onClose}>
            ×
          </button>
        </div>
        <div className="tabs" role="tablist" aria-label="Settings" onKeyDown={onTabKeyDown}>
          {tabs.map(({ tab, label, disabled }) => (
            <button
              key={tab}
              id={`${id}-${tab}`}
              type="button"
              role="tab"
              className="tab"
              aria-selected={shown === tab}
              aria-controls={`${id}-panel`}
              tabIndex={shown === tab ? 0 : -1}
              disabled={disabled}
              title={disabled ? 'Open a board to change its settings' : undefined}
              onClick={() => onTab(tab)}
            >
              {label}
            </button>
          ))}
        </div>
        <section id={`${id}-panel`} role="tabpanel" aria-labelledby={`${id}-${shown}`}>
          {shown === 'app' || !projectTab ? (
            <AppTab api={api} onPortChanged={onProjectsChanged} />
          ) : (
            <ProjectTab
              key={projectTab.id}
              project={projectTab}
              api={api}
              onRenamed={onProjectsChanged}
              onReset={onReset}
              onRemove={onRemove}
            />
          )}
        </section>
      </div>
    </main>
  )
}

/** Loads the app settings and saves your changes. */
function AppTab({ api, onPortChanged }: { api: SettingsApi; onPortChanged: () => void }) {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  // `null` until you type, so the field follows the saved port.
  const [portText, setPortText] = useState<string | null>(null)
  const [portError, setPortError] = useState<string | null>(null)
  const [themeError, setThemeError] = useState<string | null>(null)
  const [linkError, setLinkError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    let current = true
    api.getAppSettings().then(
      (settings) => current && setSettings(settings),
      (error: unknown) => current && setLoadError(errorMessage(error)),
    )
    return () => {
      current = false
    }
  }, [api])

  async function applyPort(port: number) {
    setBusy(true)
    setPortError(null)
    try {
      setSettings(await api.setMcpPort(port))
      setPortText(null)
      onPortChanged()
    } catch (error) {
      setPortError(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  async function chooseTheme(theme: Theme) {
    if (settings === null) return
    const before = settings.theme
    setThemeError(null)
    setSettings({ ...settings, theme })
    applyTheme(theme)
    try {
      setSettings(await api.setTheme(theme))
    } catch (error) {
      setSettings((settings) => settings && { ...settings, theme: before })
      applyTheme(before)
      setThemeError(errorMessage(error))
    }
  }

  async function openLink(link: Link) {
    setLinkError(null)
    try {
      await api.openLink(link)
    } catch (error) {
      setLinkError(errorMessage(error))
    }
  }

  if (settings === null) {
    return loadError ? (
      <p className="field-error" role="alert">
        {loadError}
      </p>
    ) : (
      <p className="muted">Loading…</p>
    )
  }
  return (
    <AppSettingsView
      settings={settings}
      portText={portText ?? String(settings.mcpPort)}
      portError={portError}
      themeError={themeError}
      busy={busy}
      onPortText={(text) => {
        setPortText(text)
        setPortError(null)
      }}
      onApplyPort={(port) => void applyPort(port)}
      onTheme={(theme) => void chooseTheme(theme)}
      linkError={linkError}
      onOpenLink={(link) => void openLink(link)}
    />
  )
}

/** The App tab: the MCP server's port, the theme, and the running version. */
export function AppSettingsView({
  settings,
  portText,
  portError,
  themeError,
  busy,
  onPortText,
  onApplyPort,
  onTheme,
  linkError,
  onOpenLink,
  env = BUILD_ENV,
  version = APP_VERSION,
}: {
  settings: AppSettings
  portText: string
  portError: string | null
  themeError: string | null
  busy: boolean
  onPortText: (text: string) => void
  onApplyPort: (port: number) => void
  onTheme: (theme: Theme) => void
  linkError: string | null
  onOpenLink: (link: Link) => void
  /** The build to describe in About; the running one unless a test says otherwise. */
  env?: BuildEnv
  version?: string
}) {
  const parsed = parsePort(portText)
  const about = versionText(env, version)
  const port = 'port' in parsed ? parsed.port : null
  // The same port again only makes sense to retry a server that isn't running.
  const unchanged = port === settings.mcpPort && settings.mcpUrl !== null
  const problem = 'problem' in parsed && portText.trim() !== '' ? parsed.problem : null

  return (
    <>
      <p className="hint settings-scope">For every board on this machine.</p>
      <section className="settings-section" aria-label="MCP server">
        <h3>MCP server</h3>
        {settings.mcpUrl !== null ? (
          <p className="settings-status">
            <span className="dot active" aria-hidden="true" />
            Running at <code>{settings.mcpUrl}</code>
          </p>
        ) : (
          <p className="problem">
            Not running{settings.mcpError ? `: ${settings.mcpError}` : ''}. Agents can't connect
            until it runs.
          </p>
        )}
        <form
          className="settings-row"
          onSubmit={(event) => {
            event.preventDefault()
            if (port !== null && !unchanged) onApplyPort(port)
          }}
        >
          <label className="settings-label">
            Port
            <input
              className="settings-input port-input"
              inputMode="numeric"
              spellCheck={false}
              value={portText}
              aria-invalid={problem !== null || portError !== null || undefined}
              onChange={(event) => onPortText(event.target.value)}
            />
          </label>
          <button type="submit" className="primary" disabled={busy || port === null || unchanged}>
            {busy ? 'Applying…' : 'Apply'}
          </button>
          {settings.mcpPort !== settings.defaultMcpPort && (
            <button
              type="button"
              className="link"
              disabled={busy}
              onClick={() => onApplyPort(settings.defaultMcpPort)}
            >
              Use the default, {settings.defaultMcpPort}
            </button>
          )}
        </form>
        {(problem ?? portError) && (
          <p className="field-error" role="alert">
            {problem ?? portError}
          </p>
        )}
        <p className="hint">
          Changing the port restarts the server and changes every board's MCP URL. Agents set up
          with the old URL, such as through <code>claude mcp add</code> or a committed{' '}
          <code>.mcp.json</code>, need the new one from Setup.
        </p>
      </section>
      <section className="settings-section" aria-label="Theme">
        <h3>Theme</h3>
        <div className="segmented" role="radiogroup" aria-label="Theme">
          {THEMES.map(({ theme, label }) => (
            <label key={theme} className={settings.theme === theme ? 'segment selected' : 'segment'}>
              <input
                type="radio"
                name="theme"
                value={theme}
                checked={settings.theme === theme}
                onChange={() => onTheme(theme)}
              />
              {label}
            </label>
          ))}
        </div>
        {themeError && (
          <p className="field-error" role="alert">
            {themeError}
          </p>
        )}
        <p className="hint">System follows your computer's light or dark appearance.</p>
      </section>
      <section className="settings-section" aria-label="About">
        <h3>About</h3>
        <dl className="settings-values">
          <dt>Version</dt>
          <dd>
            <code>{about}</code>
            <CopyButton text={about} label="Copy version" />
          </dd>
        </dl>
        <div className="settings-row">
          <button type="button" className="link" onClick={() => onOpenLink('releaseNotes')}>
            {env === 'dev' ? 'Releases' : 'Release notes'}
          </button>
          <button type="button" className="link" onClick={() => onOpenLink('repository')}>
            Repository
          </button>
        </div>
        {linkError && (
          <p className="field-error" role="alert">
            {linkError}
          </p>
        )}
        <p className="hint">
          Include the version when you report a bug. Links open in your browser.
        </p>
      </section>
    </>
  )
}

/** Renames the project, and runs Reset local state with its dialog. */
function ProjectTab({
  project,
  api,
  onRenamed,
  onReset,
  onRemove,
}: {
  project: ProjectInfo
  api: SettingsApi
  onRenamed: () => void
  onReset: (project: ProjectInfo) => void
  onRemove: (project: ProjectInfo) => void
}) {
  // `null` until you type, so the field follows the name, even when a
  // pull changes it.
  const [draft, setDraft] = useState<string | null>(null)
  const [nameError, setNameError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)
  const [busy, setBusy] = useState(false)
  const [resetting, setResetting] = useState(false)
  const [working, setWorking] = useState<WorkingTask[] | null>(null)
  const [resetError, setResetError] = useState<string | null>(null)

  useEffect(() => {
    if (!saved) return
    const timer = setTimeout(() => setSaved(false), 2000)
    return () => clearTimeout(timer)
  }, [saved])

  // Which tasks agents are working on, for the reset dialog's warning.
  const listAgents = api.listAgents
  useEffect(() => {
    if (!resetting) return
    let current = true
    listAgents(project.id).then(
      (agents) => current && setWorking(workingTasks(agents)),
      // Only a warning, so a failed check doesn't block the reset.
      () => current && setWorking([]),
    )
    return () => {
      current = false
    }
  }, [listAgents, project.id, resetting])

  async function saveName(name: string) {
    setBusy(true)
    setNameError(null)
    try {
      await api.setProjectName(project.id, name)
      setDraft(null)
      setSaved(true)
      onRenamed()
    } catch (error) {
      setNameError(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  async function reset() {
    setBusy(true)
    setResetError(null)
    try {
      await api.resetProject(project.id)
      setResetting(false)
      onReset(project)
    } catch (error) {
      setResetError(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <ProjectSettingsView
        project={project}
        name={draft ?? project.name}
        nameError={nameError}
        saved={saved}
        busy={busy}
        onName={(name) => {
          setDraft(name)
          setNameError(null)
          setSaved(false)
        }}
        onSaveName={(name) => void saveName(name)}
        onReset={() => {
          setWorking(null)
          setResetError(null)
          setResetting(true)
        }}
        onRemove={() => onRemove(project)}
      />
      <Dialog open={resetting} label="Reset local state" onClose={() => setResetting(false)}>
        <ResetLocalStateForm
          project={project}
          working={working}
          busy={busy}
          error={resetError}
          onCancel={() => setResetting(false)}
          onSubmit={() => void reset()}
        />
      </Dialog>
    </>
  )
}

/**
 * The Project tab: the name, which is committed in `.tandem/config.toml`,
 * then the values and actions that stay on this machine.
 */
export function ProjectSettingsView({
  project,
  name,
  nameError,
  saved,
  busy,
  onName,
  onSaveName,
  onReset,
  onRemove,
}: {
  project: ProjectInfo
  name: string
  nameError: string | null
  saved: boolean
  busy: boolean
  onName: (name: string) => void
  onSaveName: (name: string) => void
  onReset: () => void
  onRemove: () => void
}) {
  const folder = folderName(project.repoPath)
  const readOnly = project.readOnly !== null
  const problem = nameProblem(name)
  // A blank name goes back to the folder name.
  const unchanged = (name.trim() || folder) === project.name

  return (
    <>
      <p className="hint settings-scope">
        For <strong>{project.name}</strong>, in <code>{project.repoPath}</code>.
      </p>
      <section className="settings-section" aria-label="Name">
        <h3>Name</h3>
        <form
          className="settings-row"
          onSubmit={(event) => {
            event.preventDefault()
            if (!unchanged && problem === null) onSaveName(name)
          }}
        >
          <input
            className="settings-input name-input"
            aria-label="Board name"
            placeholder={folder}
            spellCheck={false}
            value={name}
            disabled={readOnly}
            aria-invalid={problem !== null || nameError !== null || undefined}
            onChange={(event) => onName(event.target.value)}
          />
          <button
            type="submit"
            className="primary"
            disabled={busy || readOnly || unchanged || problem !== null}
          >
            Save
          </button>
          {saved && (
            <span className="muted" role="status">
              Saved
            </span>
          )}
        </form>
        {(problem ?? nameError) && (
          <p className="field-error" role="alert">
            {problem ?? nameError}
          </p>
        )}
        {readOnly ? (
          <p className="hint">Read-only: {project.readOnly}</p>
        ) : (
          <p className="hint">
            Saved in <code>.tandem/config.toml</code>, so commit it and the whole team sees this
            name. Leave it empty to use the folder name, <code>{folder}</code>.
          </p>
        )}
      </section>
      <section className="settings-section" aria-label="On this machine">
        <h3>On this machine</h3>
        <p className="hint">These stay on this machine and are never committed.</p>
        <dl className="settings-values">
          <dt>Project id</dt>
          <dd>
            <code>{project.id}</code>
            <CopyButton text={project.id} label="Copy project id" />
          </dd>
          <dt>MCP URL</dt>
          <dd>
            {project.mcpUrl !== null ? (
              <>
                <code>{project.mcpUrl}</code>
                <CopyButton text={project.mcpUrl} label="Copy MCP URL" />
              </>
            ) : (
              <span className="muted">The MCP server isn't running; see the App tab.</span>
            )}
          </dd>
        </dl>
        <p className="hint">
          The id names this board's MCP endpoint and stays the same when you rename it. Every clone
          or worktree on this machine gets its own.
        </p>
      </section>
      <section className="settings-section" aria-label="Local data">
        <h3>Local data</h3>
        <div className="settings-action">
          <div>
            <h4>Reset local state</h4>
            <p className="hint">
              Deletes this board's local database and rebuilds it from the task files. Leases and
              working logs are lost.
            </p>
          </div>
          <button type="button" className="delete-task" disabled={busy} onClick={onReset}>
            Reset local state…
          </button>
        </div>
        <div className="settings-action">
          <div>
            <h4>Remove from Tandem</h4>
            <p className="hint">
              Takes the board out of the project list. The repo and its <code>.tandem/</code>{' '}
              folder stay as they are.
            </p>
          </div>
          <button type="button" className="delete-task" disabled={busy} onClick={onRemove}>
            Remove…
          </button>
        </div>
      </section>
    </>
  )
}

import { useEffect, useId, useState, type KeyboardEvent, type ReactNode } from 'react'
import type { Api, AppSettings, DeletedTask, Link, ProjectInfo, Theme } from './api.ts'
import { APP_VERSION, BUILD_ENV, type BuildEnv, versionText } from './buildEnv.ts'
import { useDeletedTasks } from './deletedTasks.ts'
import { CleanUpDeletedForm, Dialog } from './dialogs.tsx'
import { errorMessage, formatTime } from './edit.ts'
import { CopyButton } from './panels.tsx'
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
  | 'listDeletedTasks'
  | 'restoreTask'
  | 'purgeDeletedTasks'
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
  tasks,
  api,
  onTab,
  onClose,
  onProjectsChanged,
  onRemove,
}: {
  tab: SettingsTab
  project: ProjectInfo | undefined
  /** The board's task list; a change reloads the deleted tasks. */
  tasks: unknown
  api: SettingsApi
  onTab: (tab: SettingsTab) => void
  onClose: () => void
  /** A project's name or MCP URL changed, so the list needs reloading. */
  onProjectsChanged: () => void
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
              tasks={tasks}
              api={api}
              onRenamed={onProjectsChanged}
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

/** Renames the project, and lists, restores and cleans up deleted tasks. */
function ProjectTab({
  project,
  tasks,
  api,
  onRenamed,
  onRemove,
}: {
  project: ProjectInfo
  tasks: unknown
  api: SettingsApi
  onRenamed: () => void
  onRemove: (project: ProjectInfo) => void
}) {
  // `null` until you type, so the field follows the name.
  const [draft, setDraft] = useState<string | null>(null)
  const [nameError, setNameError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)
  const [busy, setBusy] = useState(false)
  const deleted = useDeletedTasks(api, project.id, tasks)

  useEffect(() => {
    if (!saved) return
    const timer = setTimeout(() => setSaved(false), 2000)
    return () => clearTimeout(timer)
  }, [saved])

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
        onRemove={() => onRemove(project)}
        deletedTasks={
          <DeletedTasksView
            tasks={deleted.tasks}
            loadError={deleted.loadError}
            busy={deleted.busy}
            error={deleted.error}
            onRestore={(id) => void deleted.actions.restore(id)}
            onCleanUp={deleted.actions.askCleanUp}
          />
        }
      />
      <Dialog
        open={deleted.confirming}
        label="Clean up deleted tasks"
        onClose={deleted.actions.cancelCleanUp}
      >
        <CleanUpDeletedForm
          count={deleted.tasks?.length ?? 0}
          busy={deleted.busy}
          error={deleted.cleanUpError}
          onCancel={deleted.actions.cancelCleanUp}
          onSubmit={() => void deleted.actions.confirmCleanUp()}
        />
      </Dialog>
    </>
  )
}

/**
 * The Project tab: the name, the MCP endpoint, the deleted tasks and
 * Remove. Everything is stored on this machine.
 */
export function ProjectSettingsView({
  project,
  name,
  nameError,
  saved,
  busy,
  onName,
  onSaveName,
  onRemove,
  deletedTasks,
}: {
  project: ProjectInfo
  name: string
  nameError: string | null
  saved: boolean
  busy: boolean
  onName: (name: string) => void
  onSaveName: (name: string) => void
  onRemove: () => void
  /** The Deleted tasks section, before Remove. */
  deletedTasks?: ReactNode
}) {
  const folder = folderName(project.repoPath)
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
            aria-invalid={problem !== null || nameError !== null || undefined}
            onChange={(event) => onName(event.target.value)}
          />
          <button
            type="submit"
            className="primary"
            disabled={busy || unchanged || problem !== null}
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
        <p className="hint">
          Stored on this machine. Leave it empty to use the folder name, <code>{folder}</code>.
        </p>
      </section>
      <section className="settings-section" aria-label="MCP endpoint">
        <h3>MCP endpoint</h3>
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
          The id names this board's MCP endpoint and stays the same when you rename it. Every
          worktree of the repo shares it.
        </p>
      </section>
      {deletedTasks}
      <section className="settings-section" aria-label="Remove from Tandem">
        <div className="settings-action">
          <div>
            <h4>Remove from Tandem</h4>
            <p className="hint">
              Deletes this board and its tasks from this machine. The folder stays as it is.
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

/**
 * The Deleted tasks section: each deleted task with Restore, and Clean up
 * deleted tasks, which asks first. `tasks` is `null` while loading.
 */
export function DeletedTasksView({
  tasks,
  loadError,
  busy,
  error,
  onRestore,
  onCleanUp,
}: {
  tasks: readonly DeletedTask[] | null
  loadError: string | null
  busy: boolean
  error: string | null
  onRestore: (id: string) => void
  onCleanUp: () => void
}) {
  return (
    <section className="settings-section" aria-label="Deleted tasks">
      <h3>Deleted tasks</h3>
      {loadError ? (
        <p className="field-error" role="alert">
          {loadError}
        </p>
      ) : tasks === null ? (
        <p className="muted">Loading…</p>
      ) : tasks.length === 0 ? (
        <p className="muted">No deleted tasks.</p>
      ) : (
        <ul className="deleted-tasks">
          {tasks.map((task) => (
            <li key={task.id}>
              <code>{task.id}</code>
              <span className="deleted-title">{task.title}</span>
              <span className="muted">deleted {formatTime(task.deletedAt)}</span>
              <button
                type="button"
                className="link"
                aria-label={`Restore ${task.id}`}
                disabled={busy}
                onClick={() => onRestore(task.id)}
              >
                Restore
              </button>
            </li>
          ))}
        </ul>
      )}
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
      <p className="hint">Deleted tasks can be restored until you clean them up.</p>
      <div className="settings-action">
        <div>
          <h4>Clean up deleted tasks</h4>
          <p className="hint">
            Deletes them and their history for good, so they can no longer be restored.
          </p>
        </div>
        <button
          type="button"
          className="delete-task"
          disabled={busy || tasks === null || tasks.length === 0}
          onClick={onCleanUp}
        >
          Clean up…
        </button>
      </div>
    </section>
  )
}

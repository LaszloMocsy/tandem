import { useEffect, useId, useState, type KeyboardEvent, type ReactNode } from 'react'
import type {
  Api,
  AppSettings,
  BackupInfo,
  DeletedTask,
  ExportFormat,
  Link,
  Place,
  ProjectInfo,
  Theme,
  UpdateChannel,
  UpdateInfo,
} from './api.ts'
import { formatSize, taskCount, useBackups } from './backups.ts'
import { APP_VERSION, BUILD_ENV, type BuildEnv, versionText } from './buildEnv.ts'
import { useDeletedTasks } from './deletedTasks.ts'
import { CleanUpDeletedForm, Dialog, RestoreBackupForm } from './dialogs.tsx'
import { errorMessage, formatTime } from './edit.ts'
import { CopyButton } from './panels.tsx'
import { fileManager, folderName, nameProblem, parsePort } from './settingsInput.ts'
import { applyTheme } from './theme.ts'

export type SettingsTab = 'app' | 'project'

/** The commands the Settings page uses. */
export type SettingsApi = Pick<
  Api,
  | 'getAppSettings'
  | 'setMcpPort'
  | 'setTheme'
  | 'setUpdateChannel'
  | 'setCheckUpdates'
  | 'checkForUpdate'
  | 'setProjectName'
  | 'listDeletedTasks'
  | 'restoreTask'
  | 'purgeDeletedTasks'
  | 'listBackups'
  | 'backUpProject'
  | 'restoreBackup'
  | 'exportTasks'
  | 'openLink'
  | 'showInFolder'
>

const CHANNELS: { channel: UpdateChannel; label: string }[] = [
  { channel: 'stable', label: 'Stable' },
  { channel: 'beta', label: 'Beta' },
]

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
  onUpdate,
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
  /** Check now found an update, for the update dialog. */
  onUpdate: (update: UpdateInfo) => void
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
            <AppTab api={api} onPortChanged={onProjectsChanged} onUpdate={onUpdate} />
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
function AppTab({
  api,
  onPortChanged,
  onUpdate,
}: {
  api: SettingsApi
  onPortChanged: () => void
  onUpdate: (update: UpdateInfo) => void
}) {
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  // `null` until you type, so the field follows the saved port.
  const [portText, setPortText] = useState<string | null>(null)
  const [portError, setPortError] = useState<string | null>(null)
  const [themeError, setThemeError] = useState<string | null>(null)
  const [linkError, setLinkError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>({ kind: 'idle' })
  const show = useShowInFolder(api)

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

  async function saveUpdates(save: () => Promise<AppSettings>) {
    setUpdateStatus({ kind: 'idle' })
    try {
      setSettings(await save())
    } catch (error) {
      setUpdateStatus({ kind: 'error', message: errorMessage(error) })
    }
  }

  async function checkNow() {
    setUpdateStatus({ kind: 'checking' })
    try {
      const found = await api.checkForUpdate(false)
      setUpdateStatus(found ? { kind: 'idle' } : { kind: 'upToDate' })
      if (found) onUpdate(found)
    } catch (error) {
      setUpdateStatus({ kind: 'error', message: errorMessage(error) })
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
      showError={show.error}
      onShow={show.show}
      updates={
        <UpdatesView
          settings={settings}
          status={updateStatus}
          onChannel={(channel) => void saveUpdates(() => api.setUpdateChannel(channel))}
          onCheckOnStart={(check) => void saveUpdates(() => api.setCheckUpdates(check))}
          onCheckNow={() => void checkNow()}
          onOpenLink={(link) => void openLink(link)}
        />
      }
    />
  )
}

/** Where Check now is. */
export type UpdateStatus =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'upToDate' }
  | { kind: 'error'; message: string }

/**
 * The Updates section: the channel, the check on start, and Check now. Dev
 * and alpha builds never update, so they only say where new builds are.
 */
export function UpdatesView({
  settings,
  status,
  onChannel,
  onCheckOnStart,
  onCheckNow,
  onOpenLink,
}: {
  settings: AppSettings
  status: UpdateStatus
  onChannel: (channel: UpdateChannel) => void
  onCheckOnStart: (check: boolean) => void
  onCheckNow: () => void
  onOpenLink: (link: Link) => void
}) {
  const channel = settings.updateChannel
  if (channel === null) {
    return (
      <section className="settings-section" aria-label="Updates">
        <h3>Updates</h3>
        <p className="hint">
          Dev and alpha builds don't update themselves. New builds are on the{' '}
          <button type="button" className="link" onClick={() => onOpenLink('releases')}>
            releases page
          </button>
          .
        </p>
      </section>
    )
  }
  return (
    <section className="settings-section" aria-label="Updates">
      <h3>Updates</h3>
      <div className="segmented" role="radiogroup" aria-label="Update channel">
        {CHANNELS.map(({ channel: value, label }) => (
          <label key={value} className={channel === value ? 'segment selected' : 'segment'}>
            <input
              type="radio"
              name="update-channel"
              value={value}
              checked={channel === value}
              onChange={() => onChannel(value)}
            />
            {label}
          </label>
        ))}
      </div>
      <label className="settings-check">
        <input
          type="checkbox"
          checked={settings.checkUpdates}
          onChange={(event) => onCheckOnStart(event.target.checked)}
        />
        Check for updates when Tandem starts
      </label>
      <div className="settings-row">
        <button type="button" disabled={status.kind === 'checking'} onClick={onCheckNow}>
          {status.kind === 'checking' ? 'Checking…' : 'Check now'}
        </button>
        {status.kind === 'upToDate' && <span className="muted">Tandem is up to date.</span>}
      </div>
      {status.kind === 'error' && (
        <p className="field-error" role="alert">
          {status.message}
        </p>
      )}
      <p className="hint">
        Stable gets production releases only. Beta also gets beta releases, whichever is newer.
        Switching to Stable from a newer beta waits for a production release newer than it.
      </p>
    </section>
  )
}

/** Shows a place in the file manager, keeping why it couldn't. */
function useShowInFolder(api: SettingsApi) {
  const [error, setError] = useState<string | null>(null)
  function show(place: Place) {
    setError(null)
    api.showInFolder(place).catch((error: unknown) => setError(errorMessage(error)))
  }
  return { error, show }
}

/**
 * A file or folder of the app's data, for a `settings-values` list: its
 * path, Copy, and a button that shows it in the file manager.
 */
function PlaceValue({
  label,
  path,
  folder = false,
  manager,
  onShow,
}: {
  label: string
  path: string
  /** Opened in the file manager, rather than shown selected. */
  folder?: boolean
  manager: string
  onShow: () => void
}) {
  return (
    <>
      <dt>{label}</dt>
      <dd>
        <code>{path}</code>
        <CopyButton text={path} label={`Copy the ${label.toLowerCase()} path`} />
        <button type="button" className="link" onClick={onShow}>
          {folder ? `Open in ${manager}` : `Show in ${manager}`}
        </button>
      </dd>
    </>
  )
}

/**
 * The App tab: the MCP server's port, the theme, where the app keeps its
 * data, and the running version.
 */
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
  showError,
  onShow,
  updates,
  env = BUILD_ENV,
  version = APP_VERSION,
  manager = fileManager(),
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
  showError: string | null
  onShow: (place: Place) => void
  /** The Updates section, before About. */
  updates?: ReactNode
  /** The build to describe in About; the running one unless a test says otherwise. */
  env?: BuildEnv
  version?: string
  /** What the file manager is called; this system's unless a test says otherwise. */
  manager?: string
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
      <section className="settings-section" aria-label="Data">
        <h3>Data</h3>
        <dl className="settings-values">
          <PlaceValue
            label="App database"
            path={settings.appDbPath}
            manager={manager}
            onShow={() => onShow({ kind: 'appDb' })}
          />
        </dl>
        {showError && (
          <p className="field-error" role="alert">
            {showError}
          </p>
        )}
        <p className="hint">
          The list of boards and these settings. Each board's database and backups are on its
          Project tab.
        </p>
      </section>
      {updates}
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

/**
 * Renames the project, backs it up and restores or exports it, and lists,
 * restores and cleans up deleted tasks.
 */
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
  const backups = useBackups(api, project.id, tasks)
  const show = useShowInFolder(api)

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
        showError={show.error}
        onShow={show.show}
        backups={
          <BackupsView
            backups={backups.backups}
            loadError={backups.loadError}
            busy={backups.busy}
            notice={backups.notice}
            error={backups.error}
            onBackUp={() => void backups.actions.backUpNow()}
            onRestore={backups.actions.askRestore}
            onExport={(format) => void backups.actions.exportTasks(format)}
          />
        }
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
      <Dialog
        open={backups.confirming !== null}
        label="Restore backup"
        onClose={backups.actions.cancelRestore}
      >
        {backups.confirming && (
          <RestoreBackupForm
            when={formatTime(backups.confirming.createdAt)}
            tasks={taskCount(backups.confirming.tasks)}
            busy={backups.busy}
            error={backups.restoreError}
            onCancel={backups.actions.cancelRestore}
            onSubmit={() => void backups.actions.confirmRestore()}
          />
        )}
      </Dialog>
    </>
  )
}

/**
 * The Project tab: the name, the MCP endpoint, where its data is, the
 * backups, the deleted tasks and Remove. Everything is stored on this
 * machine.
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
  showError,
  onShow,
  backups,
  deletedTasks,
  manager = fileManager(),
}: {
  project: ProjectInfo
  name: string
  nameError: string | null
  saved: boolean
  busy: boolean
  onName: (name: string) => void
  onSaveName: (name: string) => void
  onRemove: () => void
  showError: string | null
  onShow: (place: Place) => void
  /** The Backups section, after the data. */
  backups?: ReactNode
  /** The Deleted tasks section, before Remove. */
  deletedTasks?: ReactNode
  /** What the file manager is called; this system's unless a test says otherwise. */
  manager?: string
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
      <section className="settings-section" aria-label="Data">
        <h3>Data</h3>
        <dl className="settings-values">
          <PlaceValue
            label="Database"
            path={project.dbPath}
            manager={manager}
            onShow={() => onShow({ kind: 'projectDb', project: project.id })}
          />
          <PlaceValue
            label="Backups"
            path={project.backupsPath}
            folder
            manager={manager}
            onShow={() => onShow({ kind: 'backups', project: project.id })}
          />
        </dl>
        {showError && (
          <p className="field-error" role="alert">
            {showError}
          </p>
        )}
        <p className="hint">
          The board's tasks and their history, kept on this machine and never in the repo.
        </p>
      </section>
      {backups}
      {deletedTasks}
      <section className="settings-section" aria-label="Remove from Tandem">
        <div className="settings-action">
          <div>
            <h4>Remove from Tandem</h4>
            <p className="hint">
              Deletes this board and its tasks from this machine, keeping one last backup in case
              you open the repo again. The folder stays as it is.
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
 * The Backups section: each backup with Restore, which asks first, Back up
 * now, and Export. `backups` is `null` while loading.
 */
export function BackupsView({
  backups,
  loadError,
  busy,
  notice,
  error,
  onBackUp,
  onRestore,
  onExport,
}: {
  backups: readonly BackupInfo[] | null
  loadError: string | null
  busy: boolean
  notice: string | null
  error: string | null
  onBackUp: () => void
  onRestore: (backup: BackupInfo) => void
  onExport: (format: ExportFormat) => void
}) {
  return (
    <section className="settings-section" aria-label="Backups">
      <h3>Backups</h3>
      {loadError ? (
        <p className="field-error" role="alert">
          {loadError}
        </p>
      ) : backups === null ? (
        <p className="muted">Loading…</p>
      ) : backups.length === 0 ? (
        <p className="muted">No backups yet.</p>
      ) : (
        <ul className="backups">
          {backups.map((backup) => (
            <li key={backup.name}>
              <span className="backup-time">{formatTime(backup.createdAt)}</span>
              <span className="muted">
                {taskCount(backup.tasks)} · {formatSize(backup.size)}
              </span>
              <button
                type="button"
                className="link"
                aria-label={`Restore the backup from ${formatTime(backup.createdAt)}`}
                disabled={busy || backup.tasks === null}
                onClick={() => onRestore(backup)}
              >
                Restore…
              </button>
            </li>
          ))}
        </ul>
      )}
      {notice && (
        <p className="muted" role="status">
          {notice}
        </p>
      )}
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
      <p className="hint">
        Tandem backs up this board on this machine every hour while it changes, and when the app
        starts and quits. It keeps every backup from the last day and one a day for 30 days.
      </p>
      <div className="settings-action">
        <div>
          <h4>Back up now</h4>
          <p className="hint">Adds a backup of the board as it is.</p>
        </div>
        <button type="button" disabled={busy} onClick={onBackUp}>
          Back up now
        </button>
      </div>
      <div className="settings-action">
        <div>
          <h4>Export</h4>
          <p className="hint">
            Saves a copy to keep yourself: JSON with every task and its history, or a folder with
            each task as a Markdown file.
          </p>
        </div>
        <div className="settings-buttons">
          <button type="button" disabled={busy} onClick={() => onExport('json')}>
            JSON…
          </button>
          <button type="button" disabled={busy} onClick={() => onExport('markdown')}>
            Markdown…
          </button>
        </div>
      </div>
    </section>
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

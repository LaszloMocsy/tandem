import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent, type ReactNode } from 'react'
import type {
  Criterion,
  DeleteCheck,
  Finish,
  NewTask,
  Priority,
  ProjectInfo,
  TaskSummary,
  UpdateInfo,
} from './api.ts'
import { DependencyPicker } from './DependencyPicker.tsx'
import { formatDate, parseLines, taskName, taskOptions } from './edit.ts'
import { parseNotes } from './updates.ts'
import type { WorkingTask } from './projects.ts'

/**
 * A modal native `<dialog>`, shown while `open`. Its content mounts only
 * while it is open, so every opening starts fresh. Escape closes the
 * dialog without reaching the pane or board behind it.
 */
export function Dialog({
  open,
  label,
  onClose,
  children,
}: {
  open: boolean
  label: string
  onClose: () => void
  children: ReactNode
}) {
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const dialog = ref.current
    if (!dialog) return
    if (open && !dialog.open) dialog.showModal()
    if (!open && dialog.open) dialog.close()
  }, [open])
  return (
    <dialog
      ref={ref}
      className="dialog"
      aria-label={label}
      onClose={onClose}
      onKeyDown={(event) => {
        if (event.key === 'Escape') event.stopPropagation()
      }}
    >
      {open && children}
    </dialog>
  )
}

/** Cmd or Ctrl+Enter in a textarea submits its form. */
function submitOnModEnter(event: KeyboardEvent<HTMLTextAreaElement>) {
  if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
    event.preventDefault()
    event.currentTarget.form?.requestSubmit()
  }
}

function Actions({
  submit,
  ready,
  busy,
  onCancel,
  cancel = 'Cancel',
  danger = false,
}: {
  submit: string
  ready: boolean
  busy: boolean
  onCancel: () => void
  cancel?: string
  /** Styles the submit button as a destructive action. */
  danger?: boolean
}) {
  return (
    <div className="actions">
      <button type="submit" className={danger ? 'danger' : 'primary'} disabled={busy || !ready}>
        {submit}
      </button>
      <button type="button" disabled={busy} onClick={onCancel}>
        {cancel}
      </button>
    </div>
  )
}

function FormError({ error }: { error: string | null }) {
  if (!error) return null
  return (
    <p className="field-error" role="alert">
      {error}
    </p>
  )
}

function onSubmitted(submit: () => void) {
  return (event: FormEvent) => {
    event.preventDefault()
    submit()
  }
}

/** Review → In progress: asks for the feedback the next agent gets. */
export function ReopenForm({
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  busy: boolean
  error: string | null
  onSubmit: (feedback: string) => void
  onCancel: () => void
}) {
  const [feedback, setFeedback] = useState('')
  return (
    <form onSubmit={onSubmitted(() => onSubmit(feedback.trim()))}>
      <h3>Reopen with feedback</h3>
      <p className="hint">
        The task goes back to In progress. Your feedback is added to its changelog and given to
        the next agent that works on it.
      </p>
      <textarea
        aria-label="Feedback"
        rows={5}
        value={feedback}
        autoFocus
        disabled={busy}
        onChange={(event) => setFeedback(event.target.value)}
        onKeyDown={submitOnModEnter}
      />
      <FormError error={error} />
      <Actions submit="Reopen" ready={feedback.trim() !== ''} busy={busy} onCancel={onCancel} />
    </form>
  )
}

/**
 * In progress → Review: a summary, and for every unticked criterion either
 * a tick or the reason it isn't met, as finish_task requires.
 */
export function FinishForm({
  criteria,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  criteria: readonly Criterion[]
  busy: boolean
  error: string | null
  onSubmit: (finish: Finish) => void
  onCancel: () => void
}) {
  const [summary, setSummary] = useState('')
  const [met, setMet] = useState<ReadonlySet<number>>(new Set())
  const [reasons, setReasons] = useState<Readonly<Record<number, string>>>({})
  const open = criteria.filter((c) => !c.met)
  const unmet = open.filter((c) => !met.has(c.number))
  const ready = summary.trim() !== '' && unmet.every((c) => reasons[c.number]?.trim())

  function toggle(number: number) {
    const next = new Set(met)
    if (!next.delete(number)) next.add(number)
    setMet(next)
  }

  return (
    <form
      onSubmit={onSubmitted(() =>
        onSubmit({
          summary: summary.trim(),
          met: [...met],
          unmet: unmet.map((c) => ({ criterion: c.number, reason: reasons[c.number].trim() })),
        }),
      )}
    >
      <h3>Move to Review</h3>
      <p className="hint">Summarize the work for the changelog.</p>
      <textarea
        aria-label="Summary"
        rows={3}
        value={summary}
        autoFocus
        disabled={busy}
        onChange={(event) => setSummary(event.target.value)}
        onKeyDown={submitOnModEnter}
      />
      {open.length > 0 && (
        <>
          <p className="hint">Tick each criterion that is met, and say why for the rest.</p>
          <ul className="finish-criteria">
            {open.map((c) => (
              <li key={c.number}>
                <label>
                  <input
                    type="checkbox"
                    checked={met.has(c.number)}
                    disabled={busy}
                    onChange={() => toggle(c.number)}
                  />
                  {c.text}
                </label>
                {!met.has(c.number) && (
                  <input
                    aria-label={`Why criterion ${c.number} is not met`}
                    placeholder="Why it isn't met"
                    value={reasons[c.number] ?? ''}
                    disabled={busy}
                    onChange={(event) =>
                      setReasons({ ...reasons, [c.number]: event.target.value })
                    }
                  />
                )}
              </li>
            ))}
          </ul>
        </>
      )}
      <FormError error={error} />
      <Actions submit="Move to Review" ready={ready} busy={busy} onCancel={onCancel} />
    </form>
  )
}

/** The New task dialog. Your tasks start in Todo. */
export function NewTaskForm({
  tasks,
  defaults,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  tasks: readonly TaskSummary[]
  /** Prefilled, such as from the board's filters. */
  defaults: { label: string | null; parent: string | null }
  busy: boolean
  error: string | null
  onSubmit: (task: NewTask) => void
  onCancel: () => void
}) {
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [criteria, setCriteria] = useState('')
  const [priority, setPriority] = useState<Priority | ''>('')
  const [labels, setLabels] = useState(defaults.label ?? '')
  const [parent, setParent] = useState(defaults.parent ?? '')
  const [dependsOn, setDependsOn] = useState<string[]>([])

  function submit() {
    const task: NewTask = { title: title.trim() }
    if (description.trim()) task.description = description.trim()
    const list = parseLines(criteria)
    if (list.length > 0) task.acceptanceCriteria = list
    if (priority) task.priority = priority
    const labelList = labels
      .split(',')
      .map((label) => label.trim())
      .filter((label) => label !== '')
    if (labelList.length > 0) task.labels = [...new Set(labelList)]
    if (parent) task.parent = parent
    if (dependsOn.length > 0) task.dependsOn = dependsOn
    onSubmit(task)
  }

  return (
    <form className="new-task" onSubmit={onSubmitted(submit)}>
      <h3>New task</h3>
      <p className="hint">Your tasks start in Todo, ready for an agent.</p>
      <label className="field">
        Title
        <input value={title} autoFocus disabled={busy} onChange={(e) => setTitle(e.target.value)} />
      </label>
      <label className="field">
        Description
        <textarea
          rows={3}
          value={description}
          disabled={busy}
          onChange={(e) => setDescription(e.target.value)}
          onKeyDown={submitOnModEnter}
        />
      </label>
      <label className="field">
        <span>
          Acceptance criteria <span className="muted">· one per line</span>
        </span>
        <textarea
          rows={3}
          value={criteria}
          disabled={busy}
          onChange={(e) => setCriteria(e.target.value)}
          onKeyDown={submitOnModEnter}
        />
      </label>
      <div className="field-row">
        <label className="field">
          Priority
          <select
            value={priority}
            disabled={busy}
            onChange={(e) => setPriority(e.target.value as Priority | '')}
          >
            <option value="">None</option>
            <option value="high">High</option>
            <option value="medium">Medium</option>
            <option value="low">Low</option>
          </select>
        </label>
        <label className="field">
          <span>
            Labels <span className="muted">· comma-separated</span>
          </span>
          <input value={labels} disabled={busy} onChange={(e) => setLabels(e.target.value)} />
        </label>
      </div>
      <label className="field">
        Parent
        <select value={parent} disabled={busy} onChange={(e) => setParent(e.target.value)}>
          <option value="">None</option>
          {taskOptions(tasks, []).map((task) => (
            <option key={task.id} value={task.id}>
              {taskName(tasks, task.id)}
            </option>
          ))}
        </select>
      </label>
      <div className="field">
        Depends on
        <DependencyPicker
          value={dependsOn}
          tasks={tasks}
          exclude={[]}
          disabled={busy}
          onChange={setDependsOn}
        />
      </div>
      <FormError error={error} />
      <Actions submit="Create task" ready={title.trim() !== ''} busy={busy} onCancel={onCancel} />
    </form>
  )
}

/**
 * Opening a folder outside git, which becomes a board of its own. Folders
 * in a git repo open without asking.
 */
export function OutsideGitForm({
  repoPath,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  repoPath: string
  busy: boolean
  error: string | null
  onSubmit: () => void
  onCancel: () => void
}) {
  return (
    <form onSubmit={onSubmitted(onSubmit)}>
      <h3>Open a folder outside git?</h3>
      <p className="dialog-callout warning">
        <code>{repoPath}</code> is not in a git repository. Tandem gives every git repo one
        board, shared by all its worktrees. This folder gets a board of its own, tied to this
        path.
      </p>
      <p className="hint">
        To share a board across worktrees, run <code>git init</code> there first.
      </p>
      <FormError error={error} />
      <Actions submit="Open anyway" ready busy={busy} onCancel={onCancel} />
    </form>
  )
}

/**
 * Removing a project from the app, which never touches its folder. Blocked
 * while an agent is working on one of its tasks; `working` is `null` while
 * that is being checked.
 */
export function RemoveProjectForm({
  project,
  working,
  busy,
  error,
  onSubmit,
  onRecheck,
  onCancel,
}: {
  project: ProjectInfo
  working: readonly WorkingTask[] | null
  busy: boolean
  error: string | null
  onSubmit: () => void
  onRecheck: () => void
  onCancel: () => void
}) {
  const blocked = working !== null && working.length > 0
  return (
    <form onSubmit={onSubmitted(onSubmit)}>
      <h3>Remove “{project.name}” from Tandem?</h3>
      <p className="dialog-callout safe">
        Your codebase stays as it is. Tandem won't touch <code>{project.repoPath}</code> or any
        file in it.
      </p>
      <p className="dialog-callout danger">
        Tandem deletes this board's database from this machine: every task, its changelog and
        working log, and the deleted tasks. It keeps one last backup and deletes the others.
      </p>
      <p className="hint">
        The board leaves Tandem's project list and agents lose its MCP endpoint. Opening the
        folder again starts an empty board; restore the last backup from Settings to bring the
        tasks back.
      </p>
      {blocked && (
        <div className="dialog-callout danger" role="status">
          <p>
            An agent is working on this board. Wait until it finishes or releases{' '}
            {working.length === 1 ? 'its task' : 'these tasks'}, then remove the project.
          </p>
          <ul className="working">
            {working.map((task) => (
              <li key={task.id}>
                <code>{task.id}</code> {task.title}
              </li>
            ))}
          </ul>
          <button type="button" className="link" disabled={busy} onClick={onRecheck}>
            Check again
          </button>
        </div>
      )}
      <FormError error={blocked ? null : error} />
      <Actions
        submit={working === null ? 'Checking agents…' : 'Remove from Tandem'}
        ready={working !== null && !blocked}
        busy={busy}
        cancel="Keep board"
        danger
        onCancel={onCancel}
      />
    </form>
  )
}

/** Tasks named by id, with their titles, in a dialog callout. */
function TaskList({ ids, tasks }: { ids: readonly string[]; tasks: readonly TaskSummary[] }) {
  return (
    <ul className="working">
      {ids.map((id) => (
        <li key={id}>
          <code>{id}</code> {tasks.find((task) => task.id === id)?.title}
        </li>
      ))}
    </ul>
  )
}

/**
 * Deleting a task, which moves it to Deleted tasks until they are cleaned
 * up. Blocked while an agent works on it or another task links to it;
 * `check` is `null` while that is being checked.
 */
export function DeleteTaskForm({
  task,
  tasks,
  check,
  busy,
  error,
  onSubmit,
  onRecheck,
  onCancel,
}: {
  task: TaskSummary
  tasks: readonly TaskSummary[]
  check: DeleteCheck | null
  busy: boolean
  error: string | null
  onSubmit: () => void
  onRecheck: () => void
  onCancel: () => void
}) {
  const linked = check !== null && (check.children.length > 0 || check.dependents.length > 0)
  const blocked = check !== null && (linked || check.lease !== null)
  return (
    <form onSubmit={onSubmitted(onSubmit)}>
      <h3>Delete “{task.title || task.id}”?</h3>
      <p className="hint">
        The task leaves the board, at any status, and moves to Deleted tasks. You can restore it
        from Settings → Project until deleted tasks are cleaned up.
      </p>
      {blocked && (
        <div className="dialog-callout danger" role="status">
          {check.lease && (
            <p>
              An agent (<code>{check.lease.agentId}</code>) is working on this task. Wait until it
              finishes or releases it.
            </p>
          )}
          {check.children.length > 0 && (
            <>
              <p>It is the parent of:</p>
              <TaskList ids={check.children} tasks={tasks} />
            </>
          )}
          {check.dependents.length > 0 && (
            <>
              <p>These tasks depend on it:</p>
              <TaskList ids={check.dependents} tasks={tasks} />
            </>
          )}
          {linked && <p>Unlink them first, from their own detail panes.</p>}
          <button type="button" className="link" disabled={busy} onClick={onRecheck}>
            Check again
          </button>
        </div>
      )}
      <FormError error={blocked ? null : error} />
      <Actions
        submit={check === null && error === null ? 'Checking…' : 'Delete task'}
        ready={(check !== null || error !== null) && !blocked}
        busy={busy}
        cancel="Keep task"
        danger
        onCancel={onCancel}
      />
    </form>
  )
}

/**
 * Clean up deleted tasks: removes every deleted task for good, so none of
 * them can be restored.
 */
export function CleanUpDeletedForm({
  count,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  count: number
  busy: boolean
  error: string | null
  onSubmit: () => void
  onCancel: () => void
}) {
  const tasks = count === 1 ? '1 deleted task' : `${count} deleted tasks`
  return (
    <form onSubmit={onSubmitted(onSubmit)}>
      <h3>Clean up {tasks}?</h3>
      <p className="dialog-callout danger">
        Cleaning up permanently deletes the {tasks} and their history. This can't be undone.
      </p>
      <p className="hint">
        It also resets the undo history: after cleaning up, none of them can be restored.
      </p>
      <FormError error={error} />
      <Actions
        submit="Clean up for good"
        ready
        busy={busy}
        cancel="Keep them"
        danger
        onCancel={onCancel}
      />
    </form>
  )
}

/**
 * Restore a backup: replaces every task with the backup's. The current
 * tasks are backed up first, so it can be undone.
 */
export function RestoreBackupForm({
  when,
  tasks,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  /** When the backup was made, as shown. */
  when: string
  /** Its tasks, such as `12 tasks`. */
  tasks: string
  busy: boolean
  error: string | null
  onSubmit: () => void
  onCancel: () => void
}) {
  return (
    <form onSubmit={onSubmitted(onSubmit)}>
      <h3>Restore the backup from {when}?</h3>
      <p className="dialog-callout danger">
        Every task on this board, and its history, is replaced with the backup's {tasks}.
      </p>
      <p className="hint">
        Your current tasks are backed up first, so you can undo this by restoring that backup.
      </p>
      <FormError error={error} />
      <Actions
        submit="Restore backup"
        ready
        busy={busy}
        cancel="Keep current tasks"
        danger
        onCancel={onCancel}
      />
    </form>
  )
}

/** Release notes from Markdown: headings, lists and paragraphs. */
function ReleaseNotes({ markdown }: { markdown: string }) {
  return (
    <div className="release-notes">
      {parseNotes(markdown).map((block, index) => {
        switch (block.kind) {
          case 'heading':
            return <h4 key={index}>{block.text}</h4>
          case 'list':
            return (
              <ul key={index}>
                {block.items.map((item, i) => (
                  <li key={i}>{item}</li>
                ))}
              </ul>
            )
          case 'paragraph':
            return <p key={index}>{block.text}</p>
        }
      })}
    </div>
  )
}

/**
 * A newer version is out: its release notes, and Install and restart. An
 * install that can't replace itself, such as a `.deb`, gets Download
 * instead.
 */
export function UpdateForm({
  update,
  busy,
  error,
  onInstall,
  onDownload,
  onCancel,
}: {
  update: UpdateInfo
  busy: boolean
  error: string | null
  onInstall: () => void
  onDownload: () => void
  onCancel: () => void
}) {
  return (
    <form onSubmit={onSubmitted(update.canInstall ? onInstall : onDownload)}>
      <h3>Tandem v{update.version} is available</h3>
      {update.date && <p className="hint">Released {formatDate(update.date)}</p>}
      {update.notes && <ReleaseNotes markdown={update.notes} />}
      {update.canInstall ? (
        <p className="hint">
          Installing restarts Tandem. The MCP server stops, so connected agents lose their
          sessions until they reconnect.
        </p>
      ) : (
        <p className="hint">
          This install can't update itself. Download the new version and install it the way you
          installed this one.
        </p>
      )}
      <FormError error={error} />
      <Actions
        submit={update.canInstall ? (busy ? 'Installing…' : 'Install and restart') : 'Download'}
        ready
        busy={busy}
        onCancel={onCancel}
        cancel="Later"
      />
    </form>
  )
}

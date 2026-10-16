import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent, type ReactNode } from 'react'
import type { Criterion, Finish, NewTask, Priority, TaskSummary } from './api.ts'
import { DependencyPicker } from './DependencyPicker.tsx'
import { parseLines, taskName, taskOptions } from './edit.ts'

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
}: {
  submit: string
  ready: boolean
  busy: boolean
  onCancel: () => void
}) {
  return (
    <div className="actions">
      <button type="submit" className="primary" disabled={busy || !ready}>
        {submit}
      </button>
      <button type="button" disabled={busy} onClick={onCancel}>
        Cancel
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

import { useEffect, useState, type KeyboardEvent, type ReactNode } from 'react'
import {
  api as tauriApi,
  type Api,
  type ChangelogEntry,
  type Priority,
  type TaskDetail,
  type TaskEdit,
  type TaskSummary,
  type WorkLogEntry,
} from './api.ts'
import { COLUMNS } from './columns.ts'
import { errorMessage, formatTime, parseLines, sameList, taskName, taskOptions } from './edit.ts'
import { DependencyPicker } from './DependencyPicker.tsx'
import { ReviewBar } from './ReviewBar.tsx'

export type Field =
  | 'title'
  | 'description'
  | 'criteria'
  | 'files'
  | 'priority'
  | 'labels'
  | 'parent'
  | 'dependsOn'

/** The latest refusal per field, shown next to it. */
export type FieldErrors = Partial<Record<Field, string>>

/** Saves an edit; resolves to whether it was written. */
type Save = (field: Field, edit: TaskEdit) => Promise<boolean>

/** The commands the pane uses. */
export type PaneApi = Pick<
  Api,
  | 'getTask'
  | 'updateTask'
  | 'approve'
  | 'markReviewed'
  | 'reopen'
  | 'hasUncommittedChanges'
  | 'commitReview'
>

/**
 * The pane that slides in from the right for one task. It refetches the
 * task whenever `tasks` changes, so edits from here, agents and other
 * editors all show up, and closes on Escape.
 */
export function DetailPane({
  project,
  id,
  tasks,
  readOnly,
  onClose,
  api = tauriApi,
}: {
  project: string
  id: string
  /** The project's tasks, for the pickers and to know when to refetch. */
  tasks: readonly TaskSummary[]
  readOnly: boolean
  onClose: () => void
  api?: PaneApi
}) {
  const [detail, setDetail] = useState<TaskDetail | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [errors, setErrors] = useState<FieldErrors>({})
  const [saves, setSaves] = useState(0)
  const [uncommitted, setUncommitted] = useState(false)
  const [committed, setCommitted] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [actionError, setActionError] = useState<string | null>(null)
  const isDone = detail?.summary.status === 'done'

  useEffect(() => {
    let current = true
    api.getTask(project, id).then(
      (detail) => {
        if (!current) return
        setDetail(detail)
        setLoadError(null)
      },
      (error: unknown) => {
        if (current) setLoadError(errorMessage(error))
      },
    )
    return () => {
      current = false
    }
  }, [api, project, id, tasks, saves])

  // Whether Commit review applies. A commit made outside the app changes
  // no task file, so no event announces it; check again on window focus.
  useEffect(() => {
    if (!isDone) return
    let current = true
    const check = () =>
      api.hasUncommittedChanges(project, id).then(
        (changed) => {
          if (current) setUncommitted(changed)
        },
        // Not a git repo, or no git on PATH: nothing to commit from here.
        () => {
          if (current) setUncommitted(false)
        },
      )
    void check()
    const onFocus = () => void check()
    window.addEventListener('focus', onFocus)
    return () => {
      current = false
      window.removeEventListener('focus', onFocus)
    }
  }, [api, project, id, isDone, detail])

  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [onClose])

  const clearError = (field: Field) =>
    setErrors((errors) => ({ ...errors, [field]: undefined }))

  const save: Save = async (field, edit) => {
    try {
      await api.updateTask(project, id, edit)
      clearError(field)
      setSaves((n) => n + 1)
      return true
    } catch (error) {
      setErrors((errors) => ({ ...errors, [field]: errorMessage(error) }))
      return false
    }
  }

  /** Runs a review action; resolves to whether it succeeded. */
  async function act(action: () => Promise<unknown>): Promise<boolean> {
    setBusy(true)
    setActionError(null)
    try {
      await action()
      setSaves((n) => n + 1)
      return true
    } catch (error) {
      setActionError(errorMessage(error))
      return false
    } finally {
      setBusy(false)
    }
  }

  return (
    <aside className="detail" aria-label="Task detail">
      <button type="button" className="close" aria-label="Close" onClick={onClose}>
        ×
      </button>
      {loadError && (
        <p className="error" role="alert">
          {loadError}
        </p>
      )}
      {detail ? (
        <TaskView
          detail={detail}
          tasks={tasks}
          readOnly={readOnly}
          errors={errors}
          onSave={save}
          onClearError={clearError}
          actions={
            <ReviewBar
              status={detail.summary.status}
              disabled={readOnly || detail.content === null}
              uncommitted={uncommitted}
              committed={committed}
              busy={busy}
              error={actionError}
              onApprove={() => void act(() => api.approve(project, id))}
              onMarkReviewed={() => void act(() => api.markReviewed(project, id))}
              onReopen={(feedback) => act(() => api.reopen(project, id, feedback))}
              onCommit={() =>
                void act(async () => {
                  setCommitted(await api.commitReview(project, id))
                  setUncommitted(false)
                })
              }
            />
          }
        />
      ) : (
        !loadError && <p className="empty">Loading…</p>
      )}
    </aside>
  )
}

/** What the pane shows for a loaded task. */
export function TaskView({
  detail,
  tasks,
  readOnly,
  errors,
  onSave,
  onClearError,
  actions,
}: {
  detail: TaskDetail
  tasks: readonly TaskSummary[]
  readOnly: boolean
  errors: FieldErrors
  onSave: Save
  onClearError: (field: Field) => void
  /** Your actions on the task, under the title. */
  actions?: ReactNode
}) {
  const { summary, content } = detail
  // An unreadable file is never written, so nothing is editable.
  const locked = readOnly || content === null
  const status = COLUMNS.find((column) => column.status === summary.status)?.title
  const unmet = new Map(summary.unmet.map((u) => [u.criterion, u.reason]))

  return (
    <div className="detail-body">
      <div className="detail-top">
        <code>{summary.id}</code>
        <span className="status">{status}</span>
        {content?.reviewedAt && <span className="muted">reviewed {content.reviewedAt}</span>}
      </div>

      <Editable
        label="title"
        value={summary.title}
        disabled={locked}
        error={errors.title}
        onCancel={() => onClearError('title')}
        onSave={(draft) =>
          draft.trim() === summary.title ? done() : onSave('title', { title: draft.trim() })
        }
      >
        <h2 className="detail-title">
          {summary.title || <span className="muted">Unreadable file</span>}
        </h2>
      </Editable>

      {summary.problem && (
        <p className="problem" role="alert">
          <strong>Invalid.</strong> {summary.problem.message}
        </p>
      )}

      {actions}

      {content === null ? (
        <Section title="File">
          <pre className="raw">{detail.markdown}</pre>
        </Section>
      ) : (
        <>
          <Metadata
            summary={summary}
            tasks={tasks}
            disabled={locked}
            errors={errors}
            onSave={onSave}
          />

          <Section title="Description">
            <Editable
              label="description"
              multiline
              value={content.description}
              disabled={locked}
              error={errors.description}
              onCancel={() => onClearError('description')}
              onSave={(draft) =>
                draft.trim() === content.description
                  ? done()
                  : onSave('description', { description: draft })
              }
            >
              {content.description ? (
                <p className="prose">{content.description}</p>
              ) : (
                <p className="muted">No description.</p>
              )}
            </Editable>
          </Section>

          <Section title="Acceptance criteria">
            <Editable
              label="acceptance criteria"
              multiline
              value={content.criteria.map((c) => c.text).join('\n')}
              hint={
                content.criteria.some((c) => c.met)
                  ? 'One per line. Reworded or new criteria start unticked.'
                  : 'One per line.'
              }
              disabled={locked}
              error={errors.criteria}
              onCancel={() => onClearError('criteria')}
              onSave={(draft) => {
                const criteria = parseLines(draft)
                return sameList(
                  criteria,
                  content.criteria.map((c) => c.text),
                )
                  ? done()
                  : onSave('criteria', { acceptanceCriteria: criteria })
              }}
            >
              {content.criteria.length > 0 ? (
                <ul className="criteria">
                  {content.criteria.map((c) => (
                    <li key={c.number}>
                      <input type="checkbox" checked={c.met} readOnly disabled aria-label={c.met ? 'Met' : 'Not met'} />
                      <span>
                        {c.text}
                        {unmet.has(c.number) && (
                          <span className="unmet-reason">Unmet: {unmet.get(c.number)}</span>
                        )}
                      </span>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="muted">No acceptance criteria.</p>
              )}
            </Editable>
          </Section>

          <Section title="Relevant files">
            <Editable
              label="relevant files"
              multiline
              value={content.relevantFiles.join('\n')}
              hint="One per line."
              disabled={locked}
              error={errors.files}
              onCancel={() => onClearError('files')}
              onSave={(draft) => {
                const files = parseLines(draft)
                return sameList(files, content.relevantFiles)
                  ? done()
                  : onSave('files', { relevantFiles: files })
              }}
            >
              {content.relevantFiles.length > 0 ? (
                <ul className="files">
                  {content.relevantFiles.map((file) => (
                    <li key={file}>
                      <code>{file}</code>
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="muted">No relevant files.</p>
              )}
            </Editable>
          </Section>

          <Section title="Changelog">
            <Changelog entries={content.changelog} />
          </Section>
        </>
      )}

      <Section title="Working log" tag="local only, not committed">
        <WorkLog entries={detail.workLog} />
      </Section>
    </div>
  )
}

/** An unchanged draft: nothing to write. */
const done = () => Promise.resolve(true)

function Section({ title, tag, children }: { title: string; tag?: string; children: ReactNode }) {
  return (
    <section className="detail-section" aria-label={title}>
      <h3>
        {title}
        {tag && <span className="tag">{tag}</span>}
      </h3>
      {children}
    </section>
  )
}

/**
 * Shows `children` with an Edit button; editing swaps in an input, or a
 * textarea when `multiline`. Enter saves a single line and Cmd or Ctrl+Enter
 * a textarea; Escape cancels. A refused save keeps the draft open.
 */
function Editable({
  label,
  value,
  multiline = false,
  hint,
  disabled,
  error,
  onSave,
  onCancel,
  children,
}: {
  label: string
  /** The text the editor starts from. */
  value: string
  multiline?: boolean
  hint?: string
  disabled: boolean
  error?: string
  onSave: (draft: string) => Promise<boolean>
  onCancel: () => void
  children: ReactNode
}) {
  const [draft, setDraft] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)

  async function save() {
    if (draft === null || saving) return
    setSaving(true)
    const saved = await onSave(draft)
    setSaving(false)
    if (saved) setDraft(null)
  }

  function cancel() {
    setDraft(null)
    onCancel()
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.stopPropagation()
      cancel()
    } else if (event.key === 'Enter' && (!multiline || event.metaKey || event.ctrlKey)) {
      event.preventDefault()
      void save()
    }
  }

  if (draft === null) {
    return (
      <div className="editable">
        <div className="editable-view">
          {children}
          {!disabled && (
            <button
              type="button"
              className="link"
              aria-label={`Edit ${label}`}
              onClick={() => setDraft(value)}
            >
              Edit
            </button>
          )}
        </div>
        <FieldError message={error} />
      </div>
    )
  }

  const props = {
    value: draft,
    autoFocus: true,
    'aria-label': label,
    disabled: saving,
    onChange: (event: { target: { value: string } }) => setDraft(event.target.value),
    onKeyDown,
  }
  return (
    <div className="editable editing">
      {multiline ? (
        <textarea rows={Math.max(3, draft.split('\n').length + 1)} {...props} />
      ) : (
        <input {...props} />
      )}
      {hint && <p className="hint">{hint}</p>}
      <FieldError message={error} />
      <div className="actions">
        <button type="button" className="primary" disabled={saving} onClick={() => void save()}>
          Save
        </button>
        <button type="button" disabled={saving} onClick={cancel}>
          Cancel
        </button>
      </div>
    </div>
  )
}

function FieldError({ message }: { message?: string }) {
  if (!message) return null
  return (
    <p className="field-error" role="alert">
      {message}
    </p>
  )
}

const PRIORITIES: { value: Priority; name: string }[] = [
  { value: 'high', name: 'High' },
  { value: 'medium', name: 'Medium' },
  { value: 'low', name: 'Low' },
]

/** Priority, labels, parent and dependencies; each saves on change. */
function Metadata({
  summary,
  tasks,
  disabled,
  errors,
  onSave,
}: {
  summary: TaskSummary
  tasks: readonly TaskSummary[]
  disabled: boolean
  errors: FieldErrors
  onSave: Save
}) {
  const { id, parent, dependsOn } = summary
  const parentMissing = parent !== null && !tasks.some((task) => task.id === parent)

  return (
    <dl className="meta">
      <dt>Priority</dt>
      <dd>
        <select
          aria-label="Priority"
          value={summary.priority}
          disabled={disabled}
          onChange={(event) => void onSave('priority', { priority: event.target.value as Priority })}
        >
          {PRIORITIES.map(({ value, name }) => (
            <option key={value} value={value}>
              {name}
            </option>
          ))}
        </select>
        <FieldError message={errors.priority} />
      </dd>

      <dt>Labels</dt>
      <dd>
        <Labels
          labels={summary.labels}
          disabled={disabled}
          onChange={(labels) => onSave('labels', { labels })}
        />
        <FieldError message={errors.labels} />
      </dd>

      <dt>Parent</dt>
      <dd>
        <select
          aria-label="Parent"
          value={parent ?? ''}
          disabled={disabled}
          onChange={(event) => void onSave('parent', { parent: event.target.value || null })}
        >
          <option value="">None</option>
          {parentMissing && <option value={parent}>{parent} (missing)</option>}
          {taskOptions(tasks, [id]).map((task) => (
            <option key={task.id} value={task.id}>
              {taskName(tasks, task.id)}
            </option>
          ))}
        </select>
        <FieldError message={errors.parent} />
      </dd>

      <dt>Depends on</dt>
      <dd>
        <DependencyPicker
          value={dependsOn}
          tasks={tasks}
          exclude={[id]}
          disabled={disabled}
          onChange={(dependsOn) => void onSave('dependsOn', { dependsOn })}
        />
        <FieldError message={errors.dependsOn} />
      </dd>
    </dl>
  )
}

function Labels({
  labels,
  disabled,
  onChange,
}: {
  labels: string[]
  disabled: boolean
  onChange: (labels: string[]) => Promise<boolean>
}) {
  const [draft, setDraft] = useState('')

  async function add() {
    const label = draft.trim()
    if (label === '' || labels.includes(label)) {
      setDraft('')
    } else if (await onChange([...labels, label])) {
      setDraft('')
    }
  }

  return (
    <div className="chips-field">
      {labels.length > 0 && (
        <ul className="chips">
          {labels.map((label) => (
            <li key={label} className="chip">
              {label}
              {!disabled && (
                <button
                  type="button"
                  aria-label={`Remove label ${label}`}
                  onClick={() => void onChange(labels.filter((l) => l !== label))}
                >
                  ×
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {!disabled && (
        <input
          className="chip-input"
          aria-label="Add label"
          placeholder="Add label"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault()
              void add()
            } else if (event.key === 'Escape' && draft !== '') {
              event.stopPropagation()
              setDraft('')
            }
          }}
        />
      )}
    </div>
  )
}

function Changelog({ entries }: { entries: ChangelogEntry[] }) {
  if (entries.length === 0) return <p className="muted">No entries.</p>
  return (
    <ol className="timeline">
      {entries.map((entry, i) => (
        <li key={i}>
          <div className="timeline-head">
            <span className="event">{entry.event}</span>
            <span className="muted">
              {entry.who} · {entry.date}
            </span>
          </div>
          {entry.text && <p className="prose">{entry.text}</p>}
        </li>
      ))}
    </ol>
  )
}

function WorkLog({ entries }: { entries: WorkLogEntry[] }) {
  if (entries.length === 0) return <p className="muted">No entries.</p>
  return (
    <ol className="timeline">
      {entries.map((entry, i) => (
        <li key={i} className={`log-${entry.kind}`}>
          <div className="timeline-head">
            <span className="event">{entry.kind}</span>
            <span className="muted">
              {entry.agentId ?? 'you'} · {formatTime(entry.createdAt)}
            </span>
          </div>
          <p className="prose">{entry.message}</p>
        </li>
      ))}
    </ol>
  )
}

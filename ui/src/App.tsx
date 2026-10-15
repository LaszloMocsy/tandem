import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { api as tauriApi, type Api, type Criterion, type Status, type TaskSummary } from './api.ts'
import { Board } from './Board.tsx'
import { buildColumns, COLUMNS } from './columns.ts'
import { DetailPane, type PaneApi } from './DetailPane.tsx'
import { Dialog, FinishForm, NewTaskForm, ReopenForm } from './dialogs.tsx'
import { errorMessage } from './edit.ts'
import { moveFor } from './moves.ts'
import { shortcutFor } from './shortcuts.ts'
import { board, useBoard, type BoardStore } from './store.ts'
import { Toolbar } from './Toolbar.tsx'
import { useNow } from './useNow.ts'
import { useSearch } from './useSearch.ts'

/** How often stale leases are rechecked, since no event announces them. */
const LEASE_TICK_MS = 15_000

/** The commands the app uses. */
export type AppApi = PaneApi &
  Pick<Api, 'listTasks' | 'createTask' | 'start' | 'finish' | 'release'>

const columnName = (status: Status) => COLUMNS.find((c) => c.status === status)?.title ?? status

/** The task whose card has focus, if any. */
function focusedTask(): string | null {
  const active = document.activeElement
  return active instanceof HTMLElement
    ? (active.closest('[data-task-id]')?.getAttribute('data-task-id') ?? null)
    : null
}

function App({ store = board, api = tauriApi }: { store?: BoardStore; api?: AppApi }) {
  const { projects, project, tasks, loading, error } = useBoard((state) => state, store)
  const now = useNow(LEASE_TICK_MS)
  const current = projects.find((p) => p.id === project)
  const readOnly = current?.readOnly != null

  const [query, setQuery] = useState('')
  const [label, setLabel] = useState<string | null>(null)
  const [parent, setParent] = useState<string | null>(null)
  const matches = useSearch(api.listTasks, project, query, tasks)
  const columns = useMemo(
    () => buildColumns(tasks, { label, parent, matches }),
    [tasks, label, parent, matches],
  )
  const searchRef = useRef<HTMLInputElement>(null)

  const [selected, setSelected] = useState<string | null>(null)
  const close = useCallback(() => setSelected(null), [])
  // A task that is gone, or from another project, closes the pane.
  const open = selected !== null && tasks.some((task) => task.id === selected) ? selected : null

  // The latest refused action from the board or a shortcut.
  const [notice, setNotice] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const [finishing, setFinishing] = useState<{
    task: TaskSummary
    criteria: Criterion[]
  } | null>(null)
  const [reopening, setReopening] = useState<TaskSummary | null>(null)
  const [busy, setBusy] = useState(false)
  const [dialogError, setDialogError] = useState<string | null>(null)

  useEffect(() => {
    store.start().catch((error: unknown) => console.error('could not start the board', error))
    return () => store.stop()
  }, [store])

  /** Runs a board action, showing a refusal as the notice. */
  const run = useCallback(async (action: () => Promise<unknown>) => {
    try {
      await action()
      setNotice(null)
    } catch (error) {
      setNotice(errorMessage(error))
    }
  }, [])

  /** Runs a dialog's action; resolves to whether it succeeded. */
  async function submit(action: () => Promise<unknown>): Promise<boolean> {
    setBusy(true)
    setDialogError(null)
    try {
      await action()
      return true
    } catch (error) {
      setDialogError(errorMessage(error))
      return false
    } finally {
      setBusy(false)
    }
  }

  const openNewTask = useCallback(() => {
    setDialogError(null)
    setCreating(true)
  }, [])

  async function move(task: TaskSummary, to: Status) {
    if (project === null) return
    const id = task.id
    switch (moveFor(task.status, to)) {
      case null:
        setNotice(`A task can't move from ${columnName(task.status)} to ${columnName(to)}.`)
        return
      case 'approve':
        return run(() => api.approve(project, id))
      case 'start':
        return run(() => api.start(project, id))
      case 'release':
        return run(() => api.release(project, id))
      case 'markReviewed':
        return run(() => api.markReviewed(project, id))
      case 'reopen':
        setDialogError(null)
        setReopening(task)
        return
      case 'finish':
        return run(async () => {
          const detail = await api.getTask(project, id)
          setDialogError(null)
          setFinishing({ task, criteria: detail.content?.criteria ?? [] })
        })
    }
  }

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented || project === null) return
      if (document.querySelector('dialog[open]')) return
      const shortcut = shortcutFor(event)
      if (shortcut === 'search') {
        event.preventDefault()
        searchRef.current?.focus()
      } else if (shortcut === 'newTask' && !readOnly) {
        event.preventDefault()
        openNewTask()
      } else if (shortcut === 'markReviewed' || shortcut === 'approve') {
        // The focused card, or else the one open in the pane.
        const id = focusedTask() ?? open
        if (id === null || readOnly) return
        event.preventDefault()
        void run(() =>
          shortcut === 'approve' ? api.approve(project, id) : api.markReviewed(project, id),
        )
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [api, project, open, readOnly, run, openNewTask])

  return (
    <div className="app">
      <header className="topbar">
        <h1>Tandem</h1>
        {current && (
          <span className="project" title={current.repoPath}>
            {current.name}
          </span>
        )}
        {current?.readOnly && (
          <span className="badge" title={current.readOnly}>
            Read-only
          </span>
        )}
        {project !== null && (
          <Toolbar
            tasks={tasks}
            query={query}
            label={label}
            parent={parent}
            readOnly={readOnly}
            searchRef={searchRef}
            onQuery={setQuery}
            onLabel={setLabel}
            onParent={setParent}
            onNewTask={openNewTask}
          />
        )}
      </header>
      {error && (
        <p className="error" role="alert">
          {error.message}
        </p>
      )}
      {notice && (
        <div className="notice" role="alert">
          {notice}
          <button type="button" aria-label="Dismiss" onClick={() => setNotice(null)}>
            ×
          </button>
        </div>
      )}
      {project === null ? (
        <p className="empty">No project open.</p>
      ) : loading ? (
        <p className="empty">Loading tasks…</p>
      ) : (
        <div className="workspace">
          <Board
            columns={columns}
            now={now}
            selected={open}
            readOnly={readOnly}
            onOpen={setSelected}
            onMove={(task, to) => void move(task, to)}
          />
          {open !== null && (
            <DetailPane
              key={open}
              project={project}
              id={open}
              tasks={tasks}
              readOnly={readOnly}
              onClose={close}
              api={api}
            />
          )}
        </div>
      )}

      {project !== null && (
        <>
          <Dialog open={creating} label="New task" onClose={() => setCreating(false)}>
            <NewTaskForm
              tasks={tasks}
              defaults={{ label, parent }}
              busy={busy}
              error={dialogError}
              onCancel={() => setCreating(false)}
              onSubmit={(task) =>
                void submit(async () => {
                  const written = await api.createTask(project, task)
                  setCreating(false)
                  setSelected(written.id)
                })
              }
            />
          </Dialog>
          <Dialog open={finishing !== null} label="Move to Review" onClose={() => setFinishing(null)}>
            {finishing && (
              <FinishForm
                criteria={finishing.criteria}
                busy={busy}
                error={dialogError}
                onCancel={() => setFinishing(null)}
                onSubmit={(finish) =>
                  void submit(async () => {
                    await api.finish(project, finishing.task.id, finish)
                    setFinishing(null)
                  })
                }
              />
            )}
          </Dialog>
          <Dialog
            open={reopening !== null}
            label="Reopen with feedback"
            onClose={() => setReopening(null)}
          >
            {reopening && (
              <ReopenForm
                busy={busy}
                error={dialogError}
                onCancel={() => setReopening(null)}
                onSubmit={(feedback) =>
                  void submit(async () => {
                    await api.reopen(project, reopening.id, feedback)
                    setReopening(null)
                  })
                }
              />
            )}
          </Dialog>
        </>
      )}
    </div>
  )
}

export default App

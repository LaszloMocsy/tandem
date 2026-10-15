import { useCallback, useEffect, useMemo, useState } from 'react'
import { Board } from './Board.tsx'
import { buildColumns } from './columns.ts'
import { DetailPane, type PaneApi } from './DetailPane.tsx'
import { board, useBoard, type BoardStore } from './store.ts'
import { useNow } from './useNow.ts'

/** How often stale leases are rechecked, since no event announces them. */
const LEASE_TICK_MS = 15_000

function App({
  store = board,
  api,
}: {
  store?: BoardStore
  /** For the detail pane; the desktop app's commands by default. */
  api?: PaneApi
}) {
  const { projects, project, tasks, loading, error } = useBoard((state) => state, store)
  const now = useNow(LEASE_TICK_MS)
  const columns = useMemo(() => buildColumns(tasks), [tasks])
  const current = projects.find((p) => p.id === project)
  const [selected, setSelected] = useState<string | null>(null)
  const close = useCallback(() => setSelected(null), [])
  // A task that is gone, or from another project, closes the pane.
  const open = selected !== null && tasks.some((task) => task.id === selected) ? selected : null

  useEffect(() => {
    store.start().catch((error: unknown) => console.error('could not start the board', error))
    return () => store.stop()
  }, [store])

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
      </header>
      {error && (
        <p className="error" role="alert">
          {error.message}
        </p>
      )}
      {project === null ? (
        <p className="empty">No project open.</p>
      ) : loading ? (
        <p className="empty">Loading tasks…</p>
      ) : (
        <div className="workspace">
          <Board columns={columns} now={now} selected={open} onOpen={setSelected} />
          {open !== null && (
            <DetailPane
              key={open}
              project={project}
              id={open}
              tasks={tasks}
              readOnly={current?.readOnly != null}
              onClose={close}
              api={api}
            />
          )}
        </div>
      )}
    </div>
  )
}

export default App

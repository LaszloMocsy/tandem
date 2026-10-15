import { useEffect, useMemo } from 'react'
import { Board } from './Board.tsx'
import { buildColumns } from './columns.ts'
import { board, useBoard, type BoardStore } from './store.ts'
import { useNow } from './useNow.ts'

/** How often stale leases are rechecked, since no event announces them. */
const LEASE_TICK_MS = 15_000

function App({ store = board }: { store?: BoardStore }) {
  const { projects, project, tasks, loading, error } = useBoard((state) => state, store)
  const now = useNow(LEASE_TICK_MS)
  const columns = useMemo(() => buildColumns(tasks), [tasks])
  const current = projects.find((p) => p.id === project)

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
        <Board columns={columns} now={now} />
      )}
    </div>
  )
}

export default App

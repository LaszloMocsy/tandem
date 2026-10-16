import type { Ref } from 'react'
import type { TaskSummary } from './api.ts'
import { labelsIn, parentsIn } from './columns.ts'
import { taskName } from './edit.ts'

/** Search, the label and parent filters, and New task. */
export function Toolbar({
  tasks,
  query,
  label,
  parent,
  readOnly,
  searchRef,
  onQuery,
  onLabel,
  onParent,
  onNewTask,
}: {
  tasks: readonly TaskSummary[]
  query: string
  label: string | null
  parent: string | null
  readOnly: boolean
  searchRef: Ref<HTMLInputElement>
  onQuery: (query: string) => void
  onLabel: (label: string | null) => void
  onParent: (parent: string | null) => void
  onNewTask: () => void
}) {
  const labels = labelsIn(tasks)
  const parents = parentsIn(tasks)
  const filtered = query !== '' || label !== null || parent !== null

  return (
    <div className="toolbar">
      <input
        ref={searchRef}
        type="search"
        className="search"
        aria-label="Search tasks"
        placeholder="Search  /"
        spellCheck={false}
        autoCorrect="off"
        autoCapitalize="off"
        value={query}
        onChange={(event) => onQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation()
            onQuery('')
            event.currentTarget.blur()
          }
        }}
      />
      <select
        aria-label="Filter by label"
        value={label ?? ''}
        onChange={(event) => onLabel(event.target.value || null)}
      >
        <option value="">All labels</option>
        {labels.map((label) => (
          <option key={label} value={label}>
            {label}
          </option>
        ))}
      </select>
      <select
        aria-label="Filter by parent"
        value={parent ?? ''}
        onChange={(event) => onParent(event.target.value || null)}
      >
        <option value="">Any parent</option>
        {parent !== null && !parents.some((task) => task.id === parent) && (
          <option value={parent}>{taskName(tasks, parent)}</option>
        )}
        {parents.map((task) => (
          <option key={task.id} value={task.id}>
            {taskName(tasks, task.id)}
          </option>
        ))}
      </select>
      {filtered && (
        <button
          type="button"
          className="link"
          onClick={() => {
            onQuery('')
            onLabel(null)
            onParent(null)
          }}
        >
          Clear filters
        </button>
      )}
      <button
        type="button"
        className="primary new-task-button"
        disabled={readOnly}
        title="New task (N)"
        onClick={onNewTask}
      >
        New task
      </button>
    </div>
  )
}

import type { TaskSummary } from './api.ts'
import { taskName, taskOptions } from './edit.ts'

/** Dependencies as removable chips, plus a picker to add one. */
export function DependencyPicker({
  value,
  tasks,
  exclude,
  disabled,
  onChange,
}: {
  value: readonly string[]
  tasks: readonly TaskSummary[]
  /** Tasks never offered, such as the task itself. */
  exclude: readonly string[]
  disabled: boolean
  onChange: (dependsOn: string[]) => void
}) {
  return (
    <>
      {value.length > 0 && (
        <ul className="chips">
          {value.map((dep) => (
            <li key={dep} className="chip">
              {taskName(tasks, dep)}
              {!disabled && (
                <button
                  type="button"
                  aria-label={`Remove dependency ${dep}`}
                  onClick={() => onChange(value.filter((d) => d !== dep))}
                >
                  ×
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {!disabled && (
        <select
          aria-label="Add dependency"
          value=""
          onChange={(event) => {
            const dep = event.target.value
            if (dep) onChange([...value, dep])
          }}
        >
          <option value="">Add dependency…</option>
          {taskOptions(tasks, [...exclude, ...value]).map((task) => (
            <option key={task.id} value={task.id}>
              {taskName(tasks, task.id)}
            </option>
          ))}
        </select>
      )}
    </>
  )
}

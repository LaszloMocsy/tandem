import type { DraggableAttributes, DraggableSyntheticListeners } from '@dnd-kit/core'
import type { Ref } from 'react'
import type { Priority } from './api.ts'
import { leaseState, unmetLabel, type CardData, type TaskRef } from './columns.ts'

const PRIORITY_NAME: Record<Priority, string> = {
  high: 'High priority',
  medium: 'Medium priority',
  low: 'Low priority',
}

/** `title (id)`, or just the id for a task that is not in the list. */
function describe(ref: TaskRef): string {
  return ref.title ? `${ref.title} (${ref.id})` : ref.id
}

/**
 * One task on the board. `now` decides whether its lease is still live.
 * Clicking it, or Enter while it has focus, opens it. The board passes
 * dnd-kit's `ref`, `attributes` and `listeners` to make it draggable.
 */
export function Card({
  card,
  now,
  selected = false,
  onOpen,
  ref,
  attributes,
  listeners,
  dragging = false,
  overlay = false,
}: {
  card: CardData
  now: number
  selected?: boolean
  onOpen?: (id: string) => void
  ref?: Ref<HTMLElement>
  attributes?: DraggableAttributes
  listeners?: DraggableSyntheticListeners
  /** The card stays in place, faded, while its copy is dragged. */
  dragging?: boolean
  /** The copy that follows the pointer. */
  overlay?: boolean
}) {
  const { task, parent, children, waitingOn } = card
  const lease = task.lease && leaseState(task.lease, now)
  const unmet = task.status === 'finished' ? task.unmet : []

  return (
    <article
      ref={ref}
      className={[
        'card',
        selected && 'selected',
        dragging && 'dragging',
        overlay && 'overlay',
      ]
        .filter(Boolean)
        .join(' ')}
      data-task-id={task.id}
      tabIndex={0}
      {...attributes}
      {...listeners}
      aria-label={task.title || task.id}
      aria-current={selected || undefined}
      onClick={() => onOpen?.(task.id)}
      onKeyDown={(event) => {
        listeners?.onKeyDown?.(event)
        if (event.key === 'Enter') onOpen?.(task.id)
      }}
    >
      <div className="card-top">
        <span
          className={`priority priority-${task.priority}`}
          role="img"
          aria-label={PRIORITY_NAME[task.priority]}
          title={PRIORITY_NAME[task.priority]}
        />
        <code className="card-id">{task.id}</code>
        {waitingOn.length > 0 && task.status !== 'done' && (
          <span
            className="lock"
            role="img"
            aria-label="Waiting on dependencies"
            title={`Waiting on ${waitingOn.map(describe).join(', ')}`}
          >
            <LockIcon />
          </span>
        )}
      </div>

      {parent && (
        <div className="breadcrumb" title={`Part of ${describe(parent)}`}>
          {parent.title ?? parent.id}
        </div>
      )}

      <h3 className="card-title">{task.title}</h3>

      {task.labels.length > 0 && (
        <ul className="labels" aria-label="Labels">
          {task.labels.map((label) => (
            <li key={label} className="label">
              {label}
            </li>
          ))}
        </ul>
      )}

      {(lease || task.blocked !== null || unmet.length > 0 || children) && (
        <div className="badges">
          {lease &&
            (lease.live ? (
              <span className="badge lease live" title={`Held by ${lease.agentId}`}>
                <span className="dot" />
                {lease.agentId}
              </span>
            ) : (
              <span
                className="badge lease stale"
                title={`${lease.agentId}'s lease expired ${lease.staleFor} ago; another agent can take over`}
              >
                <span className="dot" />
                {lease.agentId} · stale {lease.staleFor}
              </span>
            ))}
          {task.blocked !== null && (
            <span className="badge blocked" title={task.blocked}>
              Blocked
            </span>
          )}
          {unmet.length > 0 && (
            <span
              className="badge unmet"
              title={unmet
                .map((u) => `${u.criterion}. ${u.text}: ${u.reason}`)
                .join('\n')}
            >
              {unmetLabel(unmet.length)}
            </span>
          )}
          {children && (
            <span
              className="badge progress"
              title={`${children.done} of ${children.total} child tasks done`}
            >
              {children.done}/{children.total} done
            </span>
          )}
        </div>
      )}
    </article>
  )
}

function LockIcon() {
  return (
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
      <path
        fill="currentColor"
        d="M8 1a3.5 3.5 0 0 0-3.5 3.5V6H4a1 1 0 0 0-1 1v7a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1V7a1 1 0 0 0-1-1h-.5V4.5A3.5 3.5 0 0 0 8 1Zm2 5H6V4.5a2 2 0 1 1 4 0V6Z"
      />
    </svg>
  )
}

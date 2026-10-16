import { useState } from 'react'
import type { Status } from './api.ts'
import { Dialog, ReopenForm } from './dialogs.tsx'

/**
 * Your actions on a task, by status: Approve in Backlog; Mark reviewed and
 * Reopen with feedback in Review; Commit review in Done while the file has
 * uncommitted changes. Renders nothing for other tasks.
 */
export function ReviewBar({
  status,
  disabled,
  uncommitted,
  committed,
  busy,
  error,
  onApprove,
  onMarkReviewed,
  onReopen,
  onCommit,
}: {
  status: Status
  disabled: boolean
  /** Whether the task file has changes git hasn't committed. */
  uncommitted: boolean
  /** The hash of the review just committed from here. */
  committed: string | null
  busy: boolean
  /** The latest refused action. */
  error: string | null
  onApprove: () => void
  onMarkReviewed: () => void
  /** Resolves to whether the task was reopened. */
  onReopen: (feedback: string) => Promise<boolean>
  onCommit: () => void
}) {
  const [reopening, setReopening] = useState(false)

  async function reopen(feedback: string) {
    if (await onReopen(feedback)) setReopening(false)
  }

  let content
  if (status === 'backlog') {
    content = (
      <>
        <p>Waiting for your approval before agents can start it.</p>
        <button type="button" className="primary" disabled={disabled || busy} onClick={onApprove}>
          Approve
        </button>
      </>
    )
  } else if (status === 'finished') {
    content = (
      <>
        <p>Waiting for your review.</p>
        <div className="bar-actions">
          <button
            type="button"
            className="primary"
            disabled={disabled || busy}
            onClick={onMarkReviewed}
          >
            Mark reviewed
          </button>
          <button
            type="button"
            disabled={disabled || busy}
            onClick={() => setReopening(true)}
          >
            Reopen with feedback…
          </button>
        </div>
      </>
    )
  } else if (status === 'done' && uncommitted) {
    content = (
      <>
        <p>The review is not committed yet.</p>
        <button type="button" className="primary" disabled={disabled || busy} onClick={onCommit}>
          Commit review
        </button>
      </>
    )
  } else if (status === 'done' && committed) {
    content = (
      <p>
        Review committed as <code>{committed.slice(0, 7)}</code>.
      </p>
    )
  } else {
    return null
  }

  return (
    <div className="review-bar" role="region" aria-label="Review">
      <div className="review-bar-row">{content}</div>
      {error && !reopening && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
      {status === 'finished' && (
        <Dialog open={reopening} label="Reopen with feedback" onClose={() => setReopening(false)}>
          <ReopenForm
            busy={busy}
            error={error}
            onSubmit={(feedback) => void reopen(feedback)}
            onCancel={() => setReopening(false)}
          />
        </Dialog>
      )}
    </div>
  )
}

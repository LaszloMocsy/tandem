import { useState } from 'react'
import type { Status } from './api.ts'
import { Dialog, ReopenForm } from './dialogs.tsx'

/**
 * Your actions on a task, by status: Approve in Backlog; Mark reviewed and
 * Reopen with feedback in Review. Renders nothing for other tasks.
 */
export function ReviewBar({
  status,
  busy,
  error,
  onApprove,
  onMarkReviewed,
  onReopen,
}: {
  status: Status
  busy: boolean
  /** The latest refused action. */
  error: string | null
  onApprove: () => void
  onMarkReviewed: () => void
  /** Resolves to whether the task was reopened. */
  onReopen: (feedback: string) => Promise<boolean>
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
        <button type="button" className="primary" disabled={busy} onClick={onApprove}>
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
            disabled={busy}
            onClick={onMarkReviewed}
          >
            Mark reviewed
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => setReopening(true)}
          >
            Reopen with feedback…
          </button>
        </div>
      </>
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

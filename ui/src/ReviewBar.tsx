import { useRef, useState, type FormEvent } from 'react'
import type { Status } from './api.ts'

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
  const dialog = useRef<HTMLDialogElement>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [feedback, setFeedback] = useState('')

  async function reopen(event: FormEvent) {
    event.preventDefault()
    if (await onReopen(feedback.trim())) dialog.current?.close()
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
            onClick={() => {
              dialog.current?.showModal()
              setDialogOpen(true)
            }}
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
      {error && !dialogOpen && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
      {status === 'finished' && (
        <dialog
          ref={dialog}
          className="dialog"
          aria-label="Reopen with feedback"
          onClose={() => {
            setDialogOpen(false)
            setFeedback('')
          }}
          onKeyDown={(event) => {
            // Escape closes the dialog, not the pane behind it.
            if (event.key === 'Escape') event.stopPropagation()
          }}
        >
          <form onSubmit={(event) => void reopen(event)}>
            <h3>Reopen with feedback</h3>
            <p className="hint">
              The task goes back to In progress. Your feedback is added to its changelog and
              given to the next agent that works on it.
            </p>
            <textarea
              aria-label="Feedback"
              rows={5}
              value={feedback}
              autoFocus
              disabled={busy}
              onChange={(event) => setFeedback(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
                  event.preventDefault()
                  event.currentTarget.form?.requestSubmit()
                }
              }}
            />
            {error && dialogOpen && (
              <p className="field-error" role="alert">
                {error}
              </p>
            )}
            <div className="actions">
              <button type="submit" className="primary" disabled={busy || !feedback.trim()}>
                Reopen
              </button>
              <button type="button" disabled={busy} onClick={() => dialog.current?.close()}>
                Cancel
              </button>
            </div>
          </form>
        </dialog>
      )}
    </div>
  )
}

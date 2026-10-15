import { renderToStaticMarkup } from 'react-dom/server'
import { expect, test } from 'vitest'
import type { Status } from './api.ts'
import { ReviewBar } from './ReviewBar.tsx'

function render(
  status: Status,
  {
    disabled = false,
    uncommitted = false,
    committed = null as string | null,
    error = null as string | null,
  } = {},
) {
  return renderToStaticMarkup(
    <ReviewBar
      status={status}
      disabled={disabled}
      uncommitted={uncommitted}
      committed={committed}
      busy={false}
      error={error}
      onApprove={() => {}}
      onMarkReviewed={() => {}}
      onReopen={async () => true}
      onCommit={() => {}}
    />,
  )
}

/** The labels of the bar's buttons, outside the dialog. */
function buttons(html: string): string[] {
  const bar = html.split('<dialog')[0]
  return [...bar.matchAll(/<button[^>]*>([^<]+)<\/button>/g)].map((match) => match[1])
}

test('Backlog offers Approve', () => {
  expect(buttons(render('backlog'))).toEqual(['Approve'])
})

test('Review offers Mark reviewed and Reopen with feedback, with a dialog', () => {
  const html = render('finished')
  expect(buttons(html)).toEqual(['Mark reviewed', 'Reopen with feedback…'])
  // The dialog's form mounts when it opens.
  expect(html).toContain('<dialog class="dialog" aria-label="Reopen with feedback"></dialog>')
})

test('Done offers Commit review only while the file has uncommitted changes', () => {
  expect(buttons(render('done', { uncommitted: true }))).toEqual(['Commit review'])
  expect(render('done')).toBe('')
  expect(render('done', { committed: '0123456789abcdef' })).toContain(
    'Review committed as <code>0123456</code>.',
  )
})

test('other statuses show no bar', () => {
  expect(render('todo')).toBe('')
  expect(render('in_progress', { uncommitted: true })).toBe('')
})

test('actions are disabled for a read-only project or invalid file', () => {
  expect(render('backlog', { disabled: true })).toMatch(/disabled="">Approve/)
  expect(render('finished', { disabled: true })).toMatch(
    /disabled="">Mark reviewed.*disabled="">Reopen with feedback…/,
  )
})

test('a refused action shows in the bar', () => {
  expect(render('finished', { error: 'cannot mark reviewed task `k3f9x2m` while it is done' }))
    .toContain('<p class="field-error" role="alert">cannot mark reviewed task `k3f9x2m` while it is done</p>')
})

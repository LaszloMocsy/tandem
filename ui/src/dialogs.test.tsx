import { renderToStaticMarkup } from 'react-dom/server'
import { expect, test } from 'vitest'
import type { TaskSummary, UpdateInfo } from './api.ts'
import { FinishForm, NewTaskForm, ReopenForm, UpdateForm } from './dialogs.tsx'

const noop = () => {}

function summary(id: string, title: string): TaskSummary {
  return {
    id,
    title,
    status: 'todo',
    priority: 'medium',
    parent: null,
    labels: [],
    dependsOn: [],
    created: '2026-10-15',
    lease: null,
    blocked: null,
    unmet: [],
  }
}

test('Reopen needs feedback before it can submit', () => {
  const html = renderToStaticMarkup(
    <ReopenForm busy={false} error={null} onSubmit={noop} onCancel={noop} />,
  )
  expect(html).toContain('aria-label="Feedback"')
  expect(html).toContain('<button type="submit" class="primary" disabled="">Reopen</button>')
})

test('Finish asks about every unticked criterion only', () => {
  const html = renderToStaticMarkup(
    <FinishForm
      criteria={[
        { number: 1, met: true, text: 'Already met' },
        { number: 2, met: false, text: 'Has a test' },
      ]}
      busy={false}
      error="criteria 2 are neither met nor explained"
      onSubmit={noop}
      onCancel={noop}
    />,
  )
  expect(html).toContain('aria-label="Summary"')
  expect(html).not.toContain('Already met')
  expect(html).toContain('Has a test')
  expect(html).toContain('aria-label="Why criterion 2 is not met"')
  expect(html).toContain('<p class="field-error" role="alert">criteria 2 are neither met nor explained</p>')
  expect(html).toContain('disabled="">Move to Review</button>')
})

test('New task offers every field and takes defaults from the filters', () => {
  const tasks = [summary('a7rq4ne', 'Auth epic'), summary('p2xmq7d', 'Session store')]
  const html = renderToStaticMarkup(
    <NewTaskForm
      tasks={tasks}
      defaults={{ label: 'auth', parent: 'a7rq4ne' }}
      busy={false}
      error={null}
      onSubmit={noop}
      onCancel={noop}
    />,
  )
  expect(html).toContain('Your tasks start in Todo')
  for (const field of ['Title', 'Description', 'Acceptance criteria', 'Priority', 'Labels', 'Parent']) {
    expect(html).toMatch(new RegExp(`<label class="field">(<span>)?${field}`))
  }
  expect(html).toContain('value="auth"')
  expect(html).toMatch(/<option value="a7rq4ne" selected="">Auth epic \(a7rq4ne\)/)
  expect(html).toContain('aria-label="Add dependency"')
  // A title is required.
  expect(html).toContain('disabled="">Create task</button>')
})

const update: UpdateInfo = {
  version: '0.2.0',
  date: '2026-10-30T12:00:00Z',
  notes: "## What's Changed\n* Add updates by @someone in https://example.com/pull/1",
  canInstall: true,
}

test('an update shows its notes and warns that installing restarts the MCP server', () => {
  const html = renderToStaticMarkup(
    <UpdateForm
      update={update}
      busy={false}
      error={null}
      onInstall={noop}
      onDownload={noop}
      onCancel={noop}
    />,
  )
  expect(html).toContain('Tandem v0.2.0 is available')
  expect(html).toContain('<h4>What&#x27;s Changed</h4>')
  expect(html).toContain('<li>Add updates by @someone in https://example.com/pull/1</li>')
  expect(html).toContain('The MCP server stops')
  expect(html).toContain('>Install and restart</button>')
  expect(html).toContain('>Later</button>')
})

test('an install that cannot replace itself offers the download instead', () => {
  const html = renderToStaticMarkup(
    <UpdateForm
      update={{ ...update, notes: null, canInstall: false }}
      busy={false}
      error={null}
      onInstall={noop}
      onDownload={noop}
      onCancel={noop}
    />,
  )
  expect(html).toContain('can&#x27;t update itself')
  expect(html).toContain('>Download</button>')
  expect(html).not.toContain('Install and restart')
})

import { renderToStaticMarkup } from 'react-dom/server'
import { expect, test } from 'vitest'
import type { TaskDetail, TaskSummary } from './api.ts'
import { TaskView, type FieldErrors } from './DetailPane.tsx'
import { parseLines, sameList, taskName, taskOptions } from './edit.ts'

function summary(id: string, patch: Partial<TaskSummary> = {}): TaskSummary {
  return {
    id,
    title: `Task ${id}`,
    status: 'finished',
    priority: 'high',
    parent: null,
    labels: [],
    dependsOn: [],
    created: '2026-10-13',
    lease: null,
    blocked: null,
    problem: null,
    unmet: [],
    ...patch,
  }
}

const task = summary('k3f9x2m', {
  title: 'Fix token refresh race',
  parent: 'a7rq4ne',
  labels: ['auth', 'backend'],
  dependsOn: ['p2xmq7d'],
  unmet: [{ criterion: 2, text: 'A test covers it', reason: 'needs a staging account' }],
})
const tasks = [
  task,
  summary('a7rq4ne', { title: 'Auth epic' }),
  summary('p2xmq7d', { title: 'Session store' }),
]

const detail: TaskDetail = {
  summary: task,
  markdown: '---\nid: k3f9x2m\n---\n',
  parent: tasks[1],
  children: [],
  workLog: [
    {
      agentId: 'claude-code',
      kind: 'note',
      message: 'Reproduced with two tabs.',
      createdAt: '2026-10-14T09:00:00.000Z',
    },
    { agentId: null, kind: 'feedback', message: 'Add a test.', createdAt: '2026-10-14T10:00:00.000Z' },
  ],
  reviewFeedback: null,
  content: {
    description: 'Parallel requests each refresh.\n\nSecond paragraph.',
    criteria: [
      { number: 1, met: true, text: 'One refresh call' },
      { number: 2, met: false, text: 'A test covers it' },
    ],
    relevantFiles: ['src/auth/session.ts'],
    changelog: [
      { date: '2026-10-13', who: 'sam', event: 'created', text: '' },
      {
        date: '2026-10-14',
        who: 'claude-code',
        event: 'finished',
        text: 'Serialized refresh.\nUnmet: 2 (needs a staging account)',
      },
    ],
    reviewedAt: null,
  },
}

function render(
  data: TaskDetail = detail,
  { readOnly = false, errors = {} }: { readOnly?: boolean; errors?: FieldErrors } = {},
) {
  return renderToStaticMarkup(
    <TaskView
      detail={data}
      tasks={tasks}
      readOnly={readOnly}
      errors={errors}
      onSave={async () => true}
      onClearError={() => {}}
    />,
  )
}

/** The markup of the section labelled `title`. */
function section(html: string, title: string): string {
  const start = html.indexOf(`<section class="detail-section" aria-label="${title}">`)
  expect(start, title).toBeGreaterThanOrEqual(0)
  return html.slice(start, html.indexOf('</section>', start))
}

test('shows the spec with an Edit button per part', () => {
  const html = render()
  expect(html).toContain('<h2 class="detail-title">Fix token refresh race</h2>')
  expect(html).toContain('<span class="status">Review</span>')
  for (const label of ['title', 'description', 'acceptance criteria', 'relevant files']) {
    expect(html).toContain(`aria-label="Edit ${label}"`)
  }
  expect(section(html, 'Description')).toContain('Parallel requests each refresh.\n\nSecond paragraph.')
  const criteria = section(html, 'Acceptance criteria')
  expect(criteria).toContain('aria-label="Met" checked=""/><span>One refresh call</span>')
  expect(criteria).toContain('A test covers it<span class="unmet-reason">Unmet: needs a staging account</span>')
  expect(section(html, 'Relevant files')).toContain('<code>src/auth/session.ts</code>')
})

test('shows the metadata controls with task names', () => {
  const html = render()
  expect(html).toMatch(/aria-label="Priority"[^>]*>.*<option value="high" selected="">High/)
  expect(html).toContain('auth<button type="button" aria-label="Remove label auth">')
  expect(html).toMatch(/aria-label="Parent"[^>]*>.*<option value="a7rq4ne" selected="">Auth epic \(a7rq4ne\)/)
  expect(html).toContain('Session store (p2xmq7d)<button type="button" aria-label="Remove dependency p2xmq7d">')
  // The task itself and existing dependencies are not offered again.
  const add = html.slice(html.indexOf('aria-label="Add dependency"'))
  expect(add.slice(0, add.indexOf('</select>'))).not.toMatch(/value="(k3f9x2m|p2xmq7d)"/)
})

test('shows refused edits next to their control', () => {
  const html = render(detail, {
    errors: {
      parent: 'the parent chain would loop: k3f9x2m → a7rq4ne → k3f9x2m',
      dependsOn: 'the dependency chain would loop: k3f9x2m → p2xmq7d → k3f9x2m',
    },
  })
  expect(html).toMatch(
    /aria-label="Parent".*<\/select><p class="field-error" role="alert">the parent chain would loop/,
  )
  expect(html).toMatch(
    /aria-label="Add dependency".*<\/select><p class="field-error" role="alert">the dependency chain would loop/,
  )
})

test('shows both timelines, the working log marked local only', () => {
  const html = render()
  const changelog = section(html, 'Changelog')
  expect(changelog).toContain('<span class="event">created</span><span class="muted">sam · 2026-10-13</span>')
  expect(changelog).toContain('<p class="prose">Serialized refresh.\nUnmet: 2 (needs a staging account)</p>')
  const log = section(html, 'Working log')
  expect(log).toContain('<span class="tag">local only, not committed</span>')
  expect(log).toContain('Reproduced with two tabs.')
  expect(log).toMatch(/<li class="log-feedback">.*you · /)
})

test('a read-only project shows no editors', () => {
  const html = render(detail, { readOnly: true })
  expect(html).not.toContain('Edit ')
  expect(html).not.toContain('Remove label')
  expect(html).not.toContain('Add dependency')
  expect(html).toMatch(/aria-label="Priority" disabled=""/)
})

test('an invalid file shows its error and raw text, and is not editable', () => {
  const html = render({
    ...detail,
    summary: { ...task, title: '', problem: { kind: 'invalid_file', message: 'bad YAML' } },
    markdown: '---\nstatus: [\n---\n',
    content: null,
  })
  expect(html).toContain('<strong>Invalid.</strong> bad YAML')
  expect(html).toContain('<pre class="raw">---\nstatus: [\n---\n</pre>')
  expect(html).not.toContain('Edit ')
  expect(html).not.toContain('aria-label="Description"')
  expect(html).toContain('aria-label="Working log"')
})

test('editor helpers', () => {
  expect(parseLines('  a \n\n b\n  \n')).toEqual(['a', 'b'])
  expect(sameList(['a', 'b'], ['a', 'b'])).toBe(true)
  expect(sameList(['a', 'b'], ['b', 'a'])).toBe(false)
  expect(taskOptions(tasks, ['k3f9x2m']).map((t) => t.id)).toEqual(['a7rq4ne', 'p2xmq7d'])
  expect(taskName(tasks, 'a7rq4ne')).toBe('Auth epic (a7rq4ne)')
  expect(taskName(tasks, 'zzzzzzz')).toBe('zzzzzzz')
})

test('the footer, such as Delete task, comes after the working log', () => {
  const html = renderToStaticMarkup(
    <TaskView
      detail={detail}
      tasks={tasks}
      readOnly={false}
      errors={{}}
      onSave={async () => true}
      onClearError={() => {}}
      footer={<button type="button">Delete task…</button>}
    />,
  )
  expect(html.indexOf('Delete task…')).toBeGreaterThan(html.indexOf('aria-label="Working log"'))
})

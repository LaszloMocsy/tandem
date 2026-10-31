import { expect, test } from 'vitest'
import { parseNotes } from './updates.ts'

test("GitHub's generated notes become headings, lists and paragraphs", () => {
  const notes = [
    "## What's Changed",
    '* Add updates by @someone in https://github.com/o/r/pull/1',
    '- Fix the [board](https://example.com) by @other',
    '',
    '**Full Changelog**: https://github.com/o/r/compare/v0.1.0...v0.2.0',
  ].join('\n')
  expect(parseNotes(notes)).toEqual([
    { kind: 'heading', text: "What's Changed" },
    {
      kind: 'list',
      items: ['Add updates by @someone in https://github.com/o/r/pull/1', 'Fix the board by @other'],
    },
    { kind: 'paragraph', text: 'Full Changelog: https://github.com/o/r/compare/v0.1.0...v0.2.0' },
  ])
})

test('lines of a paragraph join, and blank lines split paragraphs', () => {
  expect(parseNotes('One\r\ntwo\n\nThree')).toEqual([
    { kind: 'paragraph', text: 'One two' },
    { kind: 'paragraph', text: 'Three' },
  ])
  expect(parseNotes('')).toEqual([])
})

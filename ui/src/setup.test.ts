import { expect, test } from 'vitest'
import spec from '../../docs/spec.md?raw'
import { AGENTS_SNIPPET, claudeCommand, mcpJson } from './setup.ts'

const URL = 'http://127.0.0.1:4317/p/tandem/mcp'

test('the AGENTS.md snippet is the one in the spec', () => {
  expect(spec).toContain('```markdown\n' + AGENTS_SNIPPET + '```')
})

test('the configs match the spec with the project URL filled in', () => {
  expect(claudeCommand(URL)).toBe(`claude mcp add --transport http tandem ${URL}`)
  const placeholder = 'http://127.0.0.1:4317/p/<project-id>/mcp'
  expect(spec).toContain(claudeCommand(placeholder))
  expect(spec).toContain('```json\n' + mcpJson(placeholder) + '```')
  expect(JSON.parse(mcpJson(URL))).toEqual({
    mcpServers: { tandem: { type: 'http', url: URL } },
  })
})

import { expect, test } from 'vitest'
import { AGENTS_SNIPPET, claudeCommand, mcpJson } from './setup.ts'

const URL = 'http://127.0.0.1:4317/p/tandem/mcp'

test('the AGENTS.md snippet points agents at the tandem MCP server', () => {
  expect(AGENTS_SNIPPET.startsWith('## Tasks\n\n')).toBe(true)
  expect(AGENTS_SNIPPET).toContain(
    "This repo's tasks live in the Tandem app; agents work on them through the `tandem` MCP server.",
  )
  expect(AGENTS_SNIPPET).toContain('- If the tandem server is unreachable, ask me to open the app.\n')
  expect(AGENTS_SNIPPET).not.toContain('.tandem')
  expect(AGENTS_SNIPPET.endsWith('\n')).toBe(true)
})

test('the configs have the project URL filled in', () => {
  expect(claudeCommand(URL)).toBe(`claude mcp add --transport http tandem ${URL}`)
  expect(JSON.parse(mcpJson(URL))).toEqual({
    mcpServers: { tandem: { type: 'http', url: URL } },
  })
})

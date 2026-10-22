import { expect, test } from 'vitest'
import { folderName, MAX_NAME_LENGTH, nameProblem, parsePort } from './settingsInput.ts'

test('ports are whole numbers from 1 to 65535', () => {
  expect(parsePort(' 4400 ')).toEqual({ port: 4400 })
  expect(parsePort('65535')).toEqual({ port: 65535 })
  for (const text of ['', 'abc', '43.5', '-1', '0x10']) {
    expect(parsePort(text)).toEqual({ problem: 'Enter a port number, such as 4317.' })
  }
  for (const text of ['0', '65536']) {
    expect(parsePort(text)).toEqual({ problem: 'The port must be between 1 and 65535.' })
  }
})

test('the folder name is the last part of the repo path', () => {
  expect(folderName('/repos/tandem')).toBe('tandem')
  expect(folderName('/repos/tandem/')).toBe('tandem')
  expect(folderName('C:\\code\\tandem')).toBe('tandem')
})

test('names are checked as the app checks them', () => {
  expect(nameProblem('  Tandem  ')).toBeNull()
  expect(nameProblem('é'.repeat(MAX_NAME_LENGTH))).toBeNull()
  expect(nameProblem('a'.repeat(MAX_NAME_LENGTH + 1))).toMatch(/longer than 100/)
  expect(nameProblem('two\nlines')).toMatch(/line breaks/)
})

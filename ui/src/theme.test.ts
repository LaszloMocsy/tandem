import { expect, test } from 'vitest'
import { applyTheme, storedTheme, type ThemeTarget } from './theme.ts'

function target(stored: Record<string, string> = {}): ThemeTarget {
  return {
    root: { dataset: {} },
    storage: {
      getItem: (key) => stored[key] ?? null,
      setItem: (key, value) => {
        stored[key] = value
      },
    },
  }
}

test('light and dark set data-theme, and system removes it', () => {
  const page = target()
  applyTheme('dark', page)
  expect(page.root.dataset.theme).toBe('dark')
  applyTheme('light', page)
  expect(page.root.dataset.theme).toBe('light')
  applyTheme('system', page)
  expect(page.root.dataset.theme).toBeUndefined()
})

test('the theme is remembered for the next launch', () => {
  const page = target()
  expect(storedTheme(page)).toBe('system')
  applyTheme('dark', page)
  expect(storedTheme(page)).toBe('dark')
  expect(storedTheme(target({ 'tandem.theme': 'sepia' }))).toBe('system')
  expect(storedTheme({ root: { dataset: {} }, storage: null })).toBe('system')
})

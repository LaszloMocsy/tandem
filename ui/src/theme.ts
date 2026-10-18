// The colour theme. The page follows `data-theme` on <html>: `light` or
// `dark` force it, and no attribute follows the system. The choice is also
// kept in localStorage, so the next launch paints it before the settings
// load; the setting in the app's database is the one that counts.

import type { Theme } from './api.ts'

const STORAGE_KEY = 'tandem.theme'

const THEMES: readonly Theme[] = ['system', 'light', 'dark']

/** The page's root and storage, which tests replace. */
export interface ThemeTarget {
  root: { dataset: DOMStringMap }
  storage: Pick<Storage, 'getItem' | 'setItem'> | null
}

function browser(): ThemeTarget {
  let storage: Storage | null = null
  try {
    storage = window.localStorage
  } catch {
    // Storage can be off; the theme then waits for the settings.
  }
  return { root: document.documentElement, storage }
}

/** Shows the page in `theme` and remembers it for the next launch. */
export function applyTheme(theme: Theme, target: ThemeTarget = browser()) {
  if (theme === 'system') delete target.root.dataset.theme
  else target.root.dataset.theme = theme
  try {
    target.storage?.setItem(STORAGE_KEY, theme)
  } catch {
    // A full or blocked storage only costs the head start.
  }
}

/** The theme from the last launch; `system` if there is none. */
export function storedTheme(target: ThemeTarget = browser()): Theme {
  let stored: string | null = null
  try {
    stored = target.storage?.getItem(STORAGE_KEY) ?? null
  } catch {
    return 'system'
  }
  return THEMES.find((theme) => theme === stored) ?? 'system'
}

// The board's single-key shortcuts, and Cmd or Ctrl+, for Settings. Enter,
// which opens the focused card, is handled by the card itself.

export type Shortcut = 'newTask' | 'search' | 'markReviewed' | 'approve'

const KEYS: Record<string, Shortcut> = {
  n: 'newTask',
  '/': 'search',
  r: 'markReviewed',
  a: 'approve',
}

/** What a key press does, if anything. Typing in a field never counts. */
export function shortcutFor(event: {
  key: string
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
  target: EventTarget | null
}): Shortcut | null {
  if (event.metaKey || event.ctrlKey || event.altKey) return null
  if (isEditable(event.target)) return null
  return KEYS[event.key.toLowerCase()] ?? null
}

/** Cmd or Ctrl+, opens Settings, as in most Mac apps, even while typing. */
export function isSettingsShortcut(event: {
  key: string
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
}): boolean {
  return (event.metaKey || event.ctrlKey) && !event.altKey && event.key === ','
}

function isEditable(target: EventTarget | null): boolean {
  if (typeof HTMLElement === 'undefined' || !(target instanceof HTMLElement)) return false
  return (
    target.isContentEditable ||
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement
  )
}

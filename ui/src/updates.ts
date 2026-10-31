// Updates: the check on start, the update it finds, and installing it. The
// desktop app does the checking and installing; this keeps what the update
// dialog shows.

import { useCallback, useEffect, useState } from 'react'
import type { Api, UpdateInfo } from './api.ts'
import { errorMessage } from './edit.ts'

/** The commands updates use. */
export type UpdatesApi = Pick<Api, 'checkForUpdate' | 'installUpdate' | 'openLink'>

/** A block of release notes: a heading, a list or a paragraph. */
export type NoteBlock =
  | { kind: 'heading'; text: string }
  | { kind: 'list'; items: string[] }
  | { kind: 'paragraph'; text: string }

/** Inline Markdown as plain text: links keep their text, emphasis and code marks go. */
function plain(text: string): string {
  return text
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/(\*\*|__|`)/g, '')
    .trim()
}

/**
 * Release notes as blocks. GitHub's generated notes use only headings,
 * bullets and paragraphs, so that is all this reads; anything else shows
 * as text.
 */
export function parseNotes(markdown: string): NoteBlock[] {
  const blocks: NoteBlock[] = []
  let paragraph: string[] = []
  const endParagraph = () => {
    if (paragraph.length > 0) blocks.push({ kind: 'paragraph', text: plain(paragraph.join(' ')) })
    paragraph = []
  }
  for (const line of markdown.split(/\r?\n/)) {
    const heading = /^#{1,6}\s+(.*)$/.exec(line)
    const item = /^\s*[*-]\s+(.*)$/.exec(line)
    if (heading) {
      endParagraph()
      blocks.push({ kind: 'heading', text: plain(heading[1]) })
    } else if (item) {
      endParagraph()
      const last = blocks.at(-1)
      if (last?.kind === 'list') last.items.push(plain(item[1]))
      else blocks.push({ kind: 'list', items: [plain(item[1])] })
    } else if (line.trim() === '') {
      endParagraph()
    } else {
      paragraph.push(line.trim())
    }
  }
  endParagraph()
  return blocks
}

/**
 * Checks for an update once on start and keeps the one found, for the
 * update dialog. A failed check on start is only logged: the app works
 * without it.
 */
export function useUpdates(api: UpdatesApi) {
  const [update, setUpdate] = useState<UpdateInfo | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let current = true
    api.checkForUpdate(true).then(
      (found) => current && found && setUpdate(found),
      (error: unknown) => console.error('could not check for an update', error),
    )
    return () => {
      current = false
    }
  }, [api])

  /** Shows an update found by a check from Settings. */
  const show = useCallback((found: UpdateInfo) => {
    setError(null)
    setUpdate(found)
  }, [])

  const close = useCallback(() => {
    setUpdate(null)
    setError(null)
  }, [])

  /** Installs the update; the app restarts into it, so this only returns on failure. */
  const install = useCallback(async () => {
    setBusy(true)
    setError(null)
    try {
      await api.installUpdate()
    } catch (error) {
      setError(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }, [api])

  const download = useCallback(async () => {
    setError(null)
    try {
      await api.openLink('download')
    } catch (error) {
      setError(errorMessage(error))
    }
  }, [api])

  return { update, busy, error, show, close, install, download }
}

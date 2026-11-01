// The Settings page's checks, as plain functions so they can be tested
// without the desktop app.

import type { Feature } from './api.ts'
import type { BuildEnv } from './buildEnv.ts'

/** Whether Settings shows Labs: in every build but production, which keeps
 * every feature off. */
export function showsLabs(env: BuildEnv): boolean {
  return env !== 'production'
}

/** The features Labs lists, in order, with what each turns on. */
export const LAB_FEATURES: readonly { feature: Feature; label: string; description: string }[] = [
  {
    feature: 'structure',
    label: 'Structure',
    description: 'Milestones, epics, dependencies and the new statuses on the board.',
  },
  {
    feature: 'canvas',
    label: 'Canvas',
    description: 'A second view that shows and edits the same structure.',
  },
  {
    feature: 'assistant',
    label: 'Assistant',
    description: 'A project chat that answers from the board and the repo.',
  },
  {
    feature: 'shaping',
    label: 'Shaping',
    description: 'From a rough note to a committed plan on the canvas.',
  },
  {
    feature: 'automation',
    label: 'Automation',
    description: 'Agents ask instead of guessing, and routine moves happen on their own.',
  },
]

/** The port typed into Settings, or why it can't be used. */
export function parsePort(text: string): { port: number } | { problem: string } {
  const trimmed = text.trim()
  if (!/^\d+$/.test(trimmed)) return { problem: 'Enter a port number, such as 4317.' }
  const port = Number(trimmed)
  if (port < 1 || port > 65535) return { problem: 'The port must be between 1 and 65535.' }
  return { port }
}

/** The name a project gets without one of its own. */
export function folderName(repoPath: string): string {
  const parts = repoPath.split(/[\\/]/).filter((part) => part !== '')
  return parts.at(-1) ?? repoPath
}

/** The longest project name the app allows. */
export const MAX_NAME_LENGTH = 100

/** Why a project name can't be saved, if it can't. */
export function nameProblem(name: string): string | null {
  // Counted in characters, as the app counts them.
  if ([...name.trim()].length > MAX_NAME_LENGTH) {
    return `The name is longer than ${MAX_NAME_LENGTH} characters.`
  }
  // oxlint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f]/.test(name)) {
    return "The name can't contain line breaks or control characters."
  }
  return null
}

/** What the system's file manager is called, for buttons that open it. */
export function fileManager(userAgent = globalThis.navigator?.userAgent ?? ''): string {
  if (/Mac/.test(userAgent)) return 'Finder'
  if (/Windows/.test(userAgent)) return 'Explorer'
  return 'file manager'
}

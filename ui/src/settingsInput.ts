// The Settings page's checks, as plain functions so they can be tested
// without the desktop app.

/** The port typed into Settings, or why it can't be used. */
export function parsePort(text: string): { port: number } | { problem: string } {
  const trimmed = text.trim()
  if (!/^\d+$/.test(trimmed)) return { problem: 'Enter a port number, such as 4317.' }
  const port = Number(trimmed)
  if (port < 1 || port > 65535) return { problem: 'The port must be between 1 and 65535.' }
  return { port }
}

/** The name a project gets without one in its `config.toml`. */
export function folderName(repoPath: string): string {
  const parts = repoPath.split(/[\\/]/).filter((part) => part !== '')
  return parts.at(-1) ?? repoPath
}

/** The longest project name, as `config.toml` allows it. */
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

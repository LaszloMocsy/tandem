// Which build is running: dev, alpha, beta or production. Vite fixes it at
// build time (see vite.config.ts). Every build but production gets a coloured
// strip across the top of the window, drawn by CSS from `data-env` on <html>,
// with the label from `data-env-label` in its middle.

export type BuildEnv = 'dev' | 'alpha' | 'beta' | 'production'

declare const __TANDEM_ENV__: BuildEnv
declare const __TANDEM_VERSION__: string

export const BUILD_ENV: BuildEnv = __TANDEM_ENV__
export const APP_VERSION: string = __TANDEM_VERSION__

/** The strip's text, such as `ALPHA build (v0.2.0-alpha.1)`. Dev builds
 * leave out the version, which only names the last release. */
export function buildEnvLabel(env: BuildEnv, version: string): string {
  const label = `${env.toUpperCase()} build`
  return env === 'dev' ? label : `${label} (v${version})`
}

/** The version line in Settings → About, such as `Tandem v0.2.0` or
 * `Tandem v0.2.0-beta.1 (BETA build)`, ready to paste into a bug report. */
export function versionText(env: BuildEnv, version: string): string {
  const text = `Tandem v${version}`
  return env === 'production' ? text : `${text} (${env.toUpperCase()} build)`
}

/** Marks the page with `env` and its label, or clears both in production. */
export function applyBuildEnv(
  env: BuildEnv = BUILD_ENV,
  version: string = APP_VERSION,
  root: { dataset: DOMStringMap } = document.documentElement,
) {
  if (env === 'production') {
    delete root.dataset.env
    delete root.dataset.envLabel
  } else {
    root.dataset.env = env
    root.dataset.envLabel = buildEnvLabel(env, version)
  }
}

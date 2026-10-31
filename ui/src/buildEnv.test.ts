import { expect, test } from 'vitest'
import { APP_VERSION, applyBuildEnv, BUILD_ENV, type BuildEnv, versionText } from './buildEnv.ts'

function marked(env: BuildEnv, version: string): DOMStringMap {
  const root = { dataset: {} as DOMStringMap }
  applyBuildEnv(env, version, root)
  return root.dataset
}

test('dev and beta builds mark the page with the strip and its label', () => {
  expect(marked('dev', '0.1.0')).toEqual({ env: 'dev', envLabel: 'DEV build' })
  expect(marked('beta', '0.2.0-beta.3')).toEqual({
    env: 'beta',
    envLabel: 'BETA build (v0.2.0-beta.3)',
  })
})

test('production builds show no strip', () => {
  expect(marked('production', '0.2.0')).toEqual({})
  const root = { dataset: { env: 'dev', envLabel: 'DEV build' } as DOMStringMap }
  applyBuildEnv('production', '0.2.0', root)
  expect(root.dataset).toEqual({})
})

test('tests run on the dev server, so the build is dev', () => {
  expect(BUILD_ENV).toBe('dev')
  expect(APP_VERSION).toMatch(/^\d+\.\d+\.\d+/)
})

test('the version text names non-production builds', () => {
  expect(versionText('production', '0.2.0')).toBe('Tandem v0.2.0')
  expect(versionText('beta', '0.2.0-beta.3')).toBe('Tandem v0.2.0-beta.3 (BETA build)')
  expect(versionText('dev', '0.1.0')).toBe('Tandem v0.1.0 (DEV build)')
})

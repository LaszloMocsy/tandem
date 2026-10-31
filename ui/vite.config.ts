import { readFileSync } from 'node:fs'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// The build environment the strip at the top of the window shows. A dev
// server is always dev; builds read TANDEM_ENV like the desktop crate does,
// since `cargo tauri build` runs this build with the same variables. Only the
// release workflow sets it, so a local build is dev.
function buildEnv(command: 'serve' | 'build'): string {
  if (command === 'serve') return 'dev'
  const env = process.env.TANDEM_ENV ?? ''
  if (env === '') return 'dev'
  if (env === 'beta' || env === 'production') return env
  throw new Error(`TANDEM_ENV must be beta, production or unset, not ${JSON.stringify(env)}`)
}

// The app version. The release workflow sets it to the tag's before
// building, so a beta build carries its X.Y.Z-beta.N.
function appVersion(): string {
  const config = readFileSync(new URL('../desktop/tauri.conf.json', import.meta.url), 'utf8')
  return (JSON.parse(config) as { version: string }).version
}

// https://vite.dev/config/
// https://v2.tauri.app/start/frontend/vite/
export default defineConfig(({ command }) => ({
  plugins: [react()],
  define: {
    __TANDEM_ENV__: JSON.stringify(buildEnv(command)),
    __TANDEM_VERSION__: JSON.stringify(appVersion()),
  },
  // Keep Rust errors visible in the terminal that `tauri dev` shares with Vite.
  clearScreen: false,
  server: {
    // tauri.conf.json's devUrl expects this exact port.
    port: 5173,
    strictPort: true,
  },
}))

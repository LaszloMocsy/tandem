import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// https://vite.dev/config/
// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [react()],
  // Keep Rust errors visible in the terminal that `tauri dev` shares with Vite.
  clearScreen: false,
  server: {
    // tauri.conf.json's devUrl expects this exact port.
    port: 5173,
    strictPort: true,
  },
})

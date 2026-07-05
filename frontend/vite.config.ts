import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// Dev server proxies the ingest-api so the browser stays same-origin (no CORS).
// Point VITE_API_TARGET at a local ingest-api (default) or an SSH tunnel to the
// prod overlay (e.g. http://<overlay-ip>:46005).
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '')
  const target = env.VITE_API_TARGET || 'http://localhost:46005'
  return {
    plugins: [react(), tailwindcss()],
    server: {
      port: 5173,
      proxy: {
        '/v1': { target, changeOrigin: true },
        '/health': { target, changeOrigin: true },
      },
    },
  }
})

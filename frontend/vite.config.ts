import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// 브로커가 /dashboard 밑에 SPA를 서빙하므로 base를 맞춘다. dev에선 API를 브로커(8770)로 프록시.
export default defineConfig({
  plugins: [react()],
  base: '/dashboard/',
  server: {
    proxy: {
      '/dashboard/events': { target: 'http://127.0.0.1:8770', changeOrigin: true },
      '/dashboard/roster': { target: 'http://127.0.0.1:8770', changeOrigin: true },
      '/dashboard/health': { target: 'http://127.0.0.1:8770', changeOrigin: true },
      '/dashboard/search': { target: 'http://127.0.0.1:8770', changeOrigin: true },
      '/dashboard/presence-timeline': { target: 'http://127.0.0.1:8770', changeOrigin: true },
      '/dashboard/goal': { target: 'http://127.0.0.1:8770', changeOrigin: true },
      '/a2a': { target: 'http://127.0.0.1:8770', changeOrigin: true },
    },
  },
})

import { defineConfig } from 'vite'

export default defineConfig({
  server: {
    proxy: {
      '/api': 'http://localhost:3456',
    }
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  }
})

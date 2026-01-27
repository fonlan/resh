import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  build: {
    target: 'esnext',
    rollupOptions: {
      output: {
        manualChunks: {
          'vendor-react': ['react', 'react-dom'],
          'vendor-xterm': ['xterm', 'xterm-addon-fit', 'xterm-addon-webgl'],
          'vendor-tauri': ['@tauri-apps/api'],
          'vendor-utils': ['uuid', 'lucide-react'],
        }
      }
    }
  }
})

import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [
    react({
      babel: {
        plugins: ['babel-plugin-react-compiler'],
      },
    }),
    tailwindcss(),
  ],
  server: {
    port: 5173,
    strictPort: true,
  },
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

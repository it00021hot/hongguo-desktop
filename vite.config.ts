import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { tanstackRouter } from '@tanstack/router-plugin/vite';
import { fileURLToPath, URL } from 'node:url';

// Tauri 要求固定端口，端口被占用时直接失败而不是自动换端口
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [
    tanstackRouter({
      target: 'react',
      routesDirectory: './src/routes',
      generatedRouteTree: './src/routeTree.gen.ts',
    }),
    react(),
    tailwindcss(),
  ],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    target: 'esnext',
    sourcemap: false,
    rollupOptions: {
      output: {
        // Vite 8 / Rolldown 要求 manualChunks 传函数而非对象
        manualChunks(id) {
          if (!id.includes('node_modules')) return undefined;
          if (id.includes('react-dom') || id.includes('/react/')) return 'react';
          if (id.includes('@tanstack')) return 'tanstack';
          return undefined;
        },
      },
    },
  },
});

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
      routesDirectory: './src/pages',
      generatedRouteTree: './src/pages/routeTree.gen.ts',
      // 页面实现与展示组件放在域目录的 components/ 下（known-issues E2），
      // 生成物 routeTree.gen.ts 也不是路由——排除后插件不再逐个告警。
      // 插件按「相对 routesDirectory 的路径片段」做 regex test，
      // `components` 子串即可同时盖住两级嵌套与 routeTree（文件名含 Route 但
      // 以 gen.ts 结尾不受影响——实测 routeTree.gen.ts 也由该 pattern 命中）。
      routeFileIgnorePattern: 'components|routeTree',
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
        // Vite 8 / Rolldown 要求 manualChunks 传函数而非对象。
        // 大头各自成 chunk（known-issues E1）：radix-ui 组件族与 lucide
        // 图标表都在 700kB 主包里，拆出去后主包只剩业务代码。
        manualChunks(id) {
          if (!id.includes('node_modules')) return undefined;
          if (id.includes('react-dom') || id.includes('/react/')) return 'react';
          if (id.includes('@tanstack')) return 'tanstack';
          if (id.includes('radix-ui')) return 'radix';
          if (id.includes('lucide-react')) return 'icons';
          if (id.includes('zod')) return 'zod';
          return undefined;
        },
      },
    },
  },
});

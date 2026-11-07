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
      // 页面实现与展示组件放在域目录的 components/ 下，
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
    warmup: {
      // tauri dev 先起 Vite 再编译 Rust（~1min），这段窗口 Vite 纯闲置。
      // dev 首帧的大头是把 277 个源模块逐个按需转换再经 HTTP 送达 WebView2
      // （实测 webview 进来后还要等 ~60s 才挂载、期间是黑窗）——预热让
      // 转换在编译窗口内做完，webview 进来直接命中热缓存，前端秒级挂载。
      clientFiles: ['./index.html', './src/main.tsx'],
    },
    watch: {
      // target/ 在 workspace 化后位于仓库根，不排除的话 chokidar 会去盯
      // target/debug/deps 下的 exe/pdb——被运行中的进程锁住，Windows 上报 EBUSY。
      ignored: ['**/crates/hongguo-tauri-app/**', '**/target/**', '**/vendor/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  define: {
    'import.meta.env.VITE_HONGGUO_OFFLINE_QA': JSON.stringify(
      process.env.HONGGUO_OFFLINE_QA === '1' ? '1' : '',
    ),
  },
  build: {
    target: 'esnext',
    sourcemap: false,
    rollupOptions: {
      output: {
        // Vite 8 / Rolldown 要求 manualChunks 传函数而非对象。
        // 大头各自成 chunk：radix-ui 组件族与 lucide
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

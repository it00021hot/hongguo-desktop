import js from '@eslint/js';
import globals from 'globals';
import reactHooks from 'eslint-plugin-react-hooks';
import reactRefresh from 'eslint-plugin-react-refresh';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    // captures/ 是逆向用的抓包产物与提取脚本（第三方混淆代码 + 数据），
    // 不是本项目源码，lint 范围只覆盖 src 与脚本；.agents/ 是 skill 资产；
    // vendor/ 是 patch 用的第三方 crate（workspace 化后位于仓库根）；
    // target/ 是 Rust 构建产物目录——release 构建会在
    // target/release/build/**/tauri-codegen-assets/ 下生成前端资源 .js，
    // 不排除就会被当成待 lint 的源码整片报解析错误（debug 下没有该目录，
    // 只在跑过 release 构建后才暴露）。
    ignores: [
      'dist',
      'captures',
      'crates/hongguo-tauri-app',
      'vendor',
      '**/target/**',
      'routeTree.gen.ts',
      'node_modules',
      '.agents',
      '**/.nsh/**',
    ],
  },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommendedTypeChecked],
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
      // type-aware 规则（no-floating-promises / no-unsafe-* 系）需要类型信息：
      // projectService 按就近 tsconfig 匹配项目（app / node 两个），无需手工列
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      'react-refresh/only-export-components': ['warn', { allowConstantExport: true }],
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
    },
  },
  {
    // shadcn/ui 组件的标准写法：组件与 `xxxVariants` 常量同文件导出。
    // 这类文件不参与 Fast Refresh 的粒度划分，豁免以保持与官方模板一致。
    files: ['src/components/ui/**/*.tsx', 'src/components/layout/app-sidebar.tsx'],
    rules: {
      'react-refresh/only-export-components': 'off',
    },
  },
  {
    // 路由文件导出的 `Route` 由 TanStack Router 生成器消费，不是组件
    files: ['src/pages/**/*.tsx'],
    rules: {
      'react-refresh/only-export-components': 'off',
    },
  },
);

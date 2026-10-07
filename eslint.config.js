import js from '@eslint/js';
import globals from 'globals';
import reactHooks from 'eslint-plugin-react-hooks';
import reactRefresh from 'eslint-plugin-react-refresh';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    // captures/ 是逆向用的抓包产物与提取脚本（第三方混淆代码 + 数据），
    // 不是本项目源码，lint 范围只覆盖 src 与脚本
    ignores: ['dist', 'captures', 'src-tauri', 'routeTree.gen.ts', 'node_modules'],
  },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
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
    files: ['src/routes/**/*.tsx'],
    rules: {
      'react-refresh/only-export-components': 'off',
    },
  },
);

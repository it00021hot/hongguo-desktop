#!/usr/bin/env node
/**
 * 沙箱环境（agent / 受限终端）里的 Tauri dev 启动器。
 *
 * 受限环境只允许子进程写工作区：默认的 `pnpm tauri dev` 会在
 * `%LOCALAPPDATA%\com.hongguo.desktop\EBWebView`（WebView2 profile）和
 * `%TEMP%`（Turso 迁移临时文件）上被拒，WebView2 把前者报成
 * `HRESULT 0x800700AA「请求的资源在使用中」`。这里把三处都重定向到
 * `target/` 下，代码路径不变，只是 dev 的运行目录换了地方。
 *
 * 只服务受限环境；普通终端仍用 `make dev`（不清空你的真实库/设置）。
 */
import { spawn } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const target = join(root, 'target');
const dataDir = join(target, 'dev-data');
const tempDir = join(target, 'dev-temp');
for (const dir of [dataDir, tempDir]) {
  mkdirSync(dir, { recursive: true });
}

const config = join(root, 'scripts', 'tauri.dev-sandbox.json');
const args = ['tauri', 'dev', '--config', config];
const env = {
  ...process.env,
  // 沙箱外不可写：库、缓存、下载目录都落在工作区内。
  HONGGUO_DATA_DIR: dataDir,
  // Turso 迁移用 std::env::temp_dir()；%TEMP% 同样在工作区外。
  TEMP: tempDir,
  TMP: tempDir,
};

// Windows 的 pnpm 是 .cmd，必须经 cmd.exe 启动；显式拼一条命令而不是
// `shell: true` + args（后者在 Node 里已标记为 deprecated）。
const child =
  process.platform === 'win32'
    ? spawn(process.env.ComSpec ?? 'cmd.exe', ['/d', '/s', '/c', 'pnpm.cmd', ...args], {
        cwd: root,
        env,
        stdio: 'inherit',
      })
    : spawn('pnpm', args, { cwd: root, env, stdio: 'inherit' });

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
  } else {
    process.exit(code ?? 0);
  }
});

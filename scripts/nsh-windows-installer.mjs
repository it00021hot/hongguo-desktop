import { spawnSync } from 'node:child_process';
import {
  copyFileSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { auditWindowsBundle } from './nsh-native-media.mjs';

const root = dirname(dirname(fileURLToPath(import.meta.url)));

function literal(value) {
  return value
    .replaceAll('$', '$$')
    .replaceAll('"', '$\\"')
    .replaceAll('\n', '$\\n')
    .replaceAll('\r', '$\\r');
}

export function uninstallFiles(bundle) {
  const files = [];
  const directories = [];
  function visit(directory, relative) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      const name = [...relative, entry.name];
      if (entry.isDirectory()) {
        visit(path, name);
        directories.push(name.join('\\'));
      } else if (entry.isFile()) {
        files.push(name.join('\\'));
      } else {
        throw new Error(`Unsupported installer payload entry: ${path}`);
      }
    }
  }
  visit(bundle, []);
  return (
    [
      ...files.map((path) => `Delete "$INSTDIR\\${literal(path)}"`),
      ...directories.map((path) => `RMDir "$INSTDIR\\${literal(path)}"`),
    ].join('\n') + '\n'
  );
}

export function buildWindowsInstaller(bundle) {
  bundle = resolve(bundle);
  auditWindowsBundle(bundle);
  copyFileSync(join(root, 'LICENSE'), join(bundle, 'LICENSE'));
  const { version } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error(`Invalid installer version: ${version}`);
  const output = dirname(bundle);
  const generated = join(output, 'installer');
  mkdirSync(generated, { recursive: true });
  const manifest = join(generated, 'uninstall-files.nsh');
  writeFileSync(manifest, uninstallFiles(bundle));
  const installer = join(output, `Hongguo-NSH_${version}_x64-setup.exe`);
  const option = process.platform === 'win32' ? '/' : '-';
  const result = spawnSync(
    'makensis',
    [
      `${option}V2`,
      `${option}WX`,
      `${option}INPUTCHARSET`,
      'UTF8',
      `${option}DBUNDLE_DIR=${literal(bundle)}`,
      `${option}DOUTPUT_FILE=${literal(installer)}`,
      `${option}DPRODUCT_VERSION=${version}`,
      `${option}DAPP_ICON=${literal(join(root, 'assets/icons/icon.ico'))}`,
      `${option}DUNINSTALL_FILES=${literal(manifest)}`,
      join(root, 'scripts/nsh-windows-installer.nsi'),
    ],
    { encoding: 'utf8' },
  );
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(`NSIS installer failed: ${result.stdout}\n${result.stderr}`);
  if (statSync(installer).size === 0) throw new Error('NSIS produced an empty installer');
  return installer;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 3)
    throw new Error('Usage: node scripts/nsh-windows-installer.mjs <Windows bundle directory>');
  process.stdout.write(`${buildWindowsInstaller(process.argv[2])}\n`);
}

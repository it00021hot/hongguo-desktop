import { createHash } from 'node:crypto';
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join, resolve, win32 } from 'node:path';
import { spawnSync } from 'node:child_process';
import { auditMacosBundle, auditWindowsBundle } from './nsh-native-media.mjs';
import { DESKTOP_TARGETS } from './release-targets.mjs';

const [triple, directory, destination] = process.argv.slice(2);
const target = DESKTOP_TARGETS[triple];
if (!target || !directory || !destination || process.platform !== target.os)
  throw new Error(
    'Use a supported desktop triple and its native runner, package directory and release directory',
  );
const source = resolve(directory);
const output = resolve(destination);
const app = JSON.parse(readFileSync('crates/hongguo-nsh/nsh.json', 'utf8'));
const stem = `${app.name}-${app.version}-${target.platform}`;
mkdirSync(output, { recursive: true });
const artifacts = [];

function run(program, args) {
  const result = spawnSync(program, args, {
    encoding: 'utf8',
    env: { ...process.env, COPYFILE_DISABLE: '1' },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${program} failed: ${result.stderr}`);
  return result.stdout.trim();
}

function artifact(from, name) {
  if (statSync(from).size === 0) throw new Error(`Empty release artifact: ${from}`);
  copyFileSync(from, join(output, name));
  artifacts.push(name);
}

function diskImage(bundle, name) {
  const staging = mkdtempSync(join(tmpdir(), 'guava-dmg-'));
  try {
    const staged = join(staging, `${app.name}.app`);
    run('ditto', [bundle, staged]);
    run('codesign', ['--verify', '--deep', '--strict', staged]);
    symlinkSync('/Applications', join(staging, 'Applications'));
    const path = join(output, name);
    run('hdiutil', [
      'create',
      '-volname',
      app.nativeHost.displayNames['zh-CN'],
      '-srcfolder',
      staging,
      '-format',
      'UDZO',
      '-ov',
      path,
    ]);
    run('hdiutil', ['verify', path]);
    artifacts.push(name);
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
}

if (target.os === 'win32') {
  auditWindowsBundle(join(source, app.name));
  const packaged = JSON.parse(readFileSync(join(source, app.name, 'nsh/app.json'), 'utf8'));
  if (packaged.version !== app.version || packaged.id !== app.id)
    throw new Error('Packaged Windows metadata does not match the release');
  const script = readFileSync(join(source, `${app.name}.iss`), 'utf8');
  for (const expected of [
    `AppId=${app.id}`,
    `UninstallDisplayName=${app.windows.displayName}`,
    win32.join('{autoprograms}', app.windows.displayName),
    win32.join('{autodesktop}', app.windows.displayName),
    win32.join('Languages', `${app.windows.installerLanguage}.isl`),
  ]) {
    if (!script.includes(expected)) throw new Error(`Installer contract missing: ${expected}`);
  }
  artifact(join(source, `${app.name}-setup.exe`), `${stem}-setup.exe`);
  artifact(join(source, `${app.name}.zip`), `${stem}-portable.zip`);
} else {
  const bundle = join(source, `${app.name}.app`);
  auditMacosBundle(bundle, run);
  const plist = join(bundle, 'Contents/Info.plist');
  for (const [key, value] of Object.entries({
    CFBundleIdentifier: app.id,
    CFBundleShortVersionString: app.version,
    CFBundleVersion: String(app.build),
  })) {
    if (run('/usr/libexec/PlistBuddy', ['-c', `Print :${key}`, plist]) !== value)
      throw new Error(`Packaged macOS metadata mismatch: ${key}`);
  }
  run('codesign', ['--verify', '--deep', '--strict', bundle]);
  const actual = run('lipo', ['-archs', join(bundle, 'Contents/MacOS', app.name)]);
  if (actual !== target.arch) throw new Error(`Expected ${target.arch}, got ${actual}`);
  const name = `${stem}.tar.gz`;
  run('tar', ['-czf', join(output, name), '-C', source, `${app.name}.app`]);
  artifacts.push(name);
  diskImage(bundle, `${stem}.dmg`);
}
const manifest = `${stem}.json`;
writeFileSync(
  join(output, manifest),
  JSON.stringify(
    {
      version: app.version,
      build: app.build,
      id: app.id,
      triple,
      commit: run('git', ['rev-parse', 'HEAD']),
      dirty: run('git', ['status', '--porcelain', '--untracked-files=no']).length > 0,
      signing: target.os === 'win32' ? 'unsigned' : 'ad-hoc-not-notarized',
      artifacts,
    },
    null,
    2,
  ) + '\n',
);
artifacts.push(manifest);
const checksums = artifacts
  .map(
    (name) =>
      `${createHash('sha256')
        .update(readFileSync(join(output, name)))
        .digest('hex')}  ${basename(name)}`,
  )
  .join('\n');
writeFileSync(join(output, `SHA256SUMS-${target.platform}.txt`), `${checksums}\n`);
console.log(`Prepared ${target.platform}: ${artifacts.join(', ')}`);

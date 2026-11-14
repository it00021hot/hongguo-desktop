import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmSync,
  statSync,
} from 'node:fs';
import { basename, dirname, join, posix, resolve, sep, win32 } from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const MAX_ARCHIVE_BYTES = 256 * 1024 * 1024;
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');

export function readNativeSdkLock() {
  return JSON.parse(readFileSync(join(repository, 'scripts/native-sdk.sources.json'), 'utf8'));
}

function targetEntry(lock, target) {
  const entry = lock.targets?.[target];
  if (lock.schemaVersion !== 1 || !entry)
    throw new Error(`Native SDK target is not supported: ${target}`);
  return entry;
}

function relativePath(path) {
  if (
    typeof path !== 'string' ||
    !path ||
    path.includes('\\') ||
    path.includes(':') ||
    path.split('/').some((part) => !part || part === '.' || part === '..')
  )
    throw new Error(`Native SDK artifact path escapes its root: ${path}`);
  return path;
}

function verifyBytes(path, sha256, byteLength, description) {
  if (byteLength !== undefined && statSync(path).size !== byteLength)
    throw new Error(`${description} size mismatch: ${path}`);
  if (hash(readFileSync(path)) !== sha256)
    throw new Error(`${description} SHA-256 mismatch: ${path}`);
}

export function verifyNativeSdk(target, directory, lock = readNativeSdkLock()) {
  const entry = targetEntry(lock, target);
  const manifestPath = join(directory, 'sdk.json');
  verifyBytes(manifestPath, entry.manifestSha256, undefined, 'Native SDK manifest');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  if (
    manifest.schemaVersion !== 1 ||
    manifest.sdkVersion !== lock.sdkVersion ||
    manifest.target?.triple !== target ||
    manifest.recipeSha256 !== entry.recipeSha256
  )
    throw new Error(`Native SDK target/version/recipe mismatch: ${target}`);
  if (!Array.isArray(manifest.artifacts) || !manifest.artifacts.length)
    throw new Error('Native SDK artifact inventory is empty');
  const root = realpathSync(directory);
  const names = new Set();
  for (const artifact of manifest.artifacts) {
    const path = relativePath(artifact.path);
    if (!Number.isSafeInteger(artifact.byteLength) || artifact.byteLength < 0)
      throw new Error(`Native SDK artifact size is invalid: ${path}`);
    if (names.has(path)) throw new Error(`Native SDK duplicate artifact: ${path}`);
    names.add(path);
    const full = realpathSync(join(directory, path));
    if (!full.startsWith(`${root}${sep}`))
      throw new Error(`Native SDK artifact escapes its root: ${path}`);
    verifyBytes(full, artifact.sha256, artifact.byteLength, 'Native SDK artifact');
  }
  const component = (name) => {
    const path = relativePath(manifest.components?.[name]?.manifestPath);
    if (path.split('/').length !== 2 || !path.endsWith('/manifest.json'))
      throw new Error(`Native SDK component must have its own top-level directory: ${name}`);
    if (!names.has(path))
      throw new Error(`Native SDK component manifest is not authenticated: ${name}`);
    return join(directory, dirname(path));
  };
  return { manifest, prefix: component('hevc'), vvdec: component('vvdec') };
}

function runTool(command, args, capture = false) {
  const executable =
    process.platform === 'win32' && ['curl', 'tar'].includes(command)
      ? win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', `${command}.exe`)
      : command;
  const result = spawnSync(executable, args, {
    encoding: 'utf8',
    stdio: capture ? 'pipe' : 'inherit',
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.error) throw new Error(`Native SDK needs ${command}: ${result.error.message}`);
  if (result.status !== 0)
    throw new Error(`Native SDK ${command} exited with ${result.status}: ${result.stderr ?? ''}`);
  return result.stdout ?? '';
}

function downloadArchive(url, path) {
  if (!url.startsWith('https://')) throw new Error('Native SDK downloads require HTTPS');
  runTool('curl', [
    '--fail',
    '--location',
    '--proto',
    '=https',
    '--proto-redir',
    '=https',
    '--retry',
    '2',
    '--max-time',
    '180',
    '--output',
    path,
    url,
  ]);
}

function archiveFor(entry, cache, options, env) {
  if (basename(entry.assetName) !== entry.assetName)
    throw new Error('Invalid native SDK asset name');
  if (env.HONGGUO_NATIVE_SDK_ARCHIVE_DIR) {
    const path = join(env.HONGGUO_NATIVE_SDK_ARCHIVE_DIR, entry.assetName);
    verifyBytes(path, entry.sha256, entry.byteLength, 'Native SDK archive');
    return path;
  }
  if (entry.archivePath) {
    const path = realpathSync(join(repository, relativePath(entry.archivePath)));
    const root = realpathSync(join(repository, 'vendor/native-codecs'));
    if (!path.startsWith(`${root}${sep}`))
      throw new Error('Vendored codec archive escapes its directory');
    verifyBytes(path, entry.sha256, entry.byteLength, 'Vendored codec archive');
    return path;
  }
  const archive = join(cache, `${entry.sha256}.tar.gz`);
  if (existsSync(archive)) {
    if (!lstatSync(archive).isFile())
      throw new Error('Native SDK archive cache is not a regular file');
    verifyBytes(archive, entry.sha256, entry.byteLength, 'Native SDK archive');
    return archive;
  }
  const temporary = mkdtempSync(join(cache, '.download-'));
  try {
    const path = join(temporary, 'archive.tar.gz');
    (options.download ?? downloadArchive)(entry.url, path);
    verifyBytes(path, entry.sha256, entry.byteLength, 'Native SDK archive');
    renameSync(path, archive);
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
  return archive;
}

function extractArchive(archive, target, staging) {
  if (statSync(archive).size > MAX_ARCHIVE_BYTES)
    throw new Error('Native SDK archive is too large');
  const entries = runTool('tar', ['-tzf', archive], true).trim().split('\n');
  for (const line of entries) {
    const path = line.replace(/\/$/, '');
    relativePath(path);
    if (path !== target && !path.startsWith(`${target}/`))
      throw new Error(`Native SDK archive has an unexpected target path: ${path}`);
  }
  runTool('tar', ['-xzf', archive, '-C', staging]);
}

function installSdk(staged, destination, target, lock) {
  const sdk = verifyNativeSdk(target, staged, lock);
  const children = [...new Set([basename(sdk.prefix), basename(sdk.vvdec), 'sdk.json'])];
  if (children.length !== 3) throw new Error('Native SDK component roots overlap');
  const backup = mkdtempSync(join(dirname(destination), '.native-sdk-backup-'));
  const replaced = [];
  const installed = [];
  let canRemoveBackup = false;
  mkdirSync(destination, { recursive: true });
  try {
    for (const name of children) {
      const path = join(destination, name);
      if (existsSync(path)) {
        renameSync(path, join(backup, name));
        replaced.push(name);
      }
      renameSync(join(staged, name), path);
      installed.push(name);
    }
    verifyNativeSdk(target, destination, lock);
    canRemoveBackup = true;
  } catch (error) {
    for (const name of installed.reverse())
      rmSync(join(destination, name), { recursive: true, force: true });
    for (const name of replaced) renameSync(join(backup, name), join(destination, name));
    canRemoveBackup = true;
    throw error;
  } finally {
    if (canRemoveBackup) rmSync(backup, { recursive: true, force: true });
  }
  return join(destination, basename(sdk.prefix));
}

export function prepareNativeSdk(target, options = {}) {
  const lock = options.lock ?? readNativeSdkLock();
  const entry = targetEntry(lock, target);
  const env = options.env ?? process.env;
  const destination = resolve(
    options.destination ??
      env.HONGGUO_NATIVE_SDK_ROOT ??
      join(repository, 'target/native-media', target),
  );
  if (env.HONGGUO_NATIVE_SDK_ROOT) return verifyNativeSdk(target, destination, lock).prefix;
  if (existsSync(join(destination, 'sdk.json'))) {
    try {
      return verifyNativeSdk(target, destination, lock).prefix;
    } catch (error) {
      console.warn(`Native SDK cache needs replacement: ${error.message}`);
    }
  }
  const parent = dirname(destination);
  mkdirSync(parent, { recursive: true });
  const guard = join(parent, `.${target}.sdk-install.lock`);
  try {
    mkdirSync(guard);
  } catch (error) {
    if (error.code === 'EEXIST')
      throw new Error(`Another native SDK setup owns ${guard}; wait for it to finish`);
    throw error;
  }
  let staging;
  try {
    staging = mkdtempSync(join(parent, '.native-sdk-stage-'));
    const cache = options.cacheDirectory ?? join(parent, '.native-sdk-downloads');
    mkdirSync(cache, { recursive: true });
    console.log(`Preparing pinned native SDK: ${target}`);
    const archive = archiveFor(entry, cache, options, env);
    extractArchive(archive, target, staging);
    return installSdk(join(staging, target), destination, target, lock);
  } finally {
    if (staging) rmSync(staging, { recursive: true, force: true });
    rmSync(guard, { recursive: true, force: true });
  }
}

export function rustTarget(env = process.env, run = () => runTool('rustc', ['-vV'], true)) {
  if (env.CARGO_BUILD_TARGET) return env.CARGO_BUILD_TARGET;
  const target = /^host: (.+)$/m.exec(run())?.[1]?.trim();
  if (!target) throw new Error('Could not determine the Rust host target from rustc -vV');
  return target;
}

export function runtimeEnvironment(
  prefix,
  { platform = process.platform, env = process.env } = {},
) {
  const paths = platform === 'win32' ? win32 : posix;
  const result = { HONGGUO_HEVC_ROOT: prefix, HONGGUO_NATIVE_SDK_ROOT: paths.dirname(prefix) };
  if (platform === 'win32')
    result.PATH = [paths.join(prefix, 'bin'), env.PATH].filter(Boolean).join(';');
  if (platform === 'darwin')
    result.DYLD_LIBRARY_PATH = [paths.join(prefix, 'lib'), env.DYLD_LIBRARY_PATH]
      .filter(Boolean)
      .join(':');
  return result;
}

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  lstatSync,
  lutimesSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  realpathSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join, relative, sep } from 'node:path';
import { gzipSync } from 'node:zlib';

export function run(command, args, env = {}) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    env: { ...process.env, ...env },
  });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(command + ' exited ' + result.status + ': ' + result.stderr);
  return result.stdout;
}

export function digest(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}
export function jsonDigest(value) {
  return createHash('sha256').update(JSON.stringify(value)).digest('hex');
}
export function writeJson(path, value) {
  writeFileSync(path, JSON.stringify(value, null, 2) + '\n');
}

function walk(directory) {
  return readdirSync(directory, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name, 'en'))
    .flatMap((entry) => {
      if (/[\r\n]/.test(entry.name)) throw new Error('SDK paths cannot contain line breaks');
      const path = join(directory, entry.name);
      return entry.isDirectory() ? [path, ...walk(path)] : [path];
    });
}

export function inventorySdk(directory) {
  const base = realpathSync(directory);
  const paths = walk(directory).filter((path) => !lstatSync(path).isDirectory());
  if (paths.length > 16384) throw new Error('SDK inventory exceeds the file budget');
  return paths.map((path) => {
    const actual = realpathSync(path);
    if (!actual.startsWith(base + sep) || !statSync(actual).isFile())
      throw new Error('SDK artifact escapes or is not a regular file: ' + path);
    if (lstatSync(path).isSymbolicLink() && /^[/\\]|^[A-Za-z]:/.test(readlinkSync(path)))
      throw new Error('SDK alias must be relative: ' + path);
    return {
      path: relative(directory, path).replaceAll('\\', '/'),
      sha256: digest(path),
      byteLength: statSync(path).size,
    };
  });
}

export function archiveSdk(directory, output) {
  const parent = dirname(directory);
  const epoch = new Date(Date.UTC(2026, 10, 14));
  const paths = [directory, ...walk(directory)];
  inventorySdk(directory);
  for (const path of paths) {
    if (lstatSync(path).isSymbolicLink()) lutimesSync(path, epoch, epoch);
    else utimesSync(path, epoch, epoch);
  }
  mkdirSync(dirname(output), { recursive: true });
  const work = mkdtempSync(join(dirname(output), '.codec-sdk-tar-'));
  try {
    const list = join(work, 'files.txt');
    writeFileSync(
      list,
      paths.map((path) => relative(parent, path).replaceAll('\\', '/')).join('\n') + '\n',
    );
    const tar = join(work, 'sdk.tar');
    const owner =
      process.platform === 'linux'
        ? ['--owner=0', '--group=0', '--numeric-owner']
        : [
            '--uid',
            '0',
            '--gid',
            '0',
            '--uname',
            'root',
            '--gname',
            'root',
            '--no-acls',
            '--no-xattrs',
            '--no-fflags',
          ];
    run(
      'tar',
      ['--format', 'ustar', ...owner, '--no-recursion', '-cf', tar, '-C', parent, '-T', list],
      { COPYFILE_DISABLE: '1' },
    );
    writeFileSync(output, gzipSync(readFileSync(tar), { level: 9 }));
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

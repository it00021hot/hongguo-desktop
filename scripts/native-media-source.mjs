import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { source } from './native-media-config.mjs';

export function verifyArchive(path, expected = source.sha256) {
  const actual = createHash('sha256').update(readFileSync(path)).digest('hex');
  if (actual !== expected) throw new Error(`FFmpeg source SHA-256 mismatch: ${path}`);
  return actual;
}

export function validateArchiveEntries(listing) {
  const entries = listing.trim().split('\n');
  const root = `ffmpeg-${source.version}/`;
  if (entries.some((entry) => !entry.startsWith(root) || entry.split('/').includes('..'))) {
    throw new Error('Invalid FFmpeg archive: entry outside the versioned source directory');
  }
}

export function sourcePatches(os) {
  return ['vt', ...(os === 'android' ? ['mediacodec'] : [])].map((name) =>
    fileURLToPath(new URL(`./native-media-ffmpeg-${name}.patch`, import.meta.url)),
  );
}

export function prepareSource(
  cache,
  run,
  destination = join(cache, `ffmpeg-${source.version}`),
  patches = sourcePatches(),
) {
  mkdirSync(cache, { recursive: true });
  const archive = join(cache, `ffmpeg-${source.version}.tar.xz`);
  if (!existsSync(archive)) {
    download(source.url, `${archive}.partial`, run);
    verifyArchive(`${archive}.partial`);
    renameSync(`${archive}.partial`, archive);
  }
  verifyArchive(archive);
  const signature = `${archive}.asc`;
  const key = join(cache, 'ffmpeg-devel.asc');
  if (!existsSync(signature)) download(source.signatureUrl, signature, run);
  if (!existsSync(key)) download(source.keyUrl, key, run);
  const keyring = join(cache, 'keyring');
  mkdirSync(keyring, { recursive: true, mode: 0o700 });
  const gpg = ['--no-options', '--batch', '--homedir', keyring];
  run('gpg', [...gpg, '--import', key]);
  const status = run('gpg', [
    ...gpg,
    '--no-auto-key-retrieve',
    '--status-fd',
    '1',
    '--verify',
    signature,
    archive,
  ]);
  if (
    !status
      .split('\n')
      .some((line) => line.startsWith(`[GNUPG:] VALIDSIG ${source.signingFingerprint} `))
  ) {
    throw new Error('FFmpeg signature does not match the pinned release signing fingerprint');
  }
  validateArchiveEntries(run('tar', ['-tf', archive]));
  const directory = destination;
  rmSync(directory, { recursive: true, force: true });
  mkdirSync(directory, { recursive: true });
  run('tar', ['-xf', archive, '--strip-components=1', '-C', directory]);
  for (const patch of patches)
    run('patch', ['--batch', '--fuzz=0', '-p1', '-i', patch], { cwd: directory });
  return { directory, archive, signature, key, patches };
}

function download(url, destination, run) {
  run('curl', [
    '--fail',
    '--location',
    '--proto',
    '=https',
    '--proto-redir',
    '=https',
    '--max-time',
    '180',
    '--output',
    destination,
    url,
  ]);
}

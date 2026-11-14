import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const system = new Set([
  'libaaudio.so',
  'libm.so',
  'liblog.so',
  'libandroid.so',
  'libmediandk.so',
  'libdl.so',
  'libEGL.so',
  'libGLESv2.so',
  'libGLESv3.so',
  'libvulkan.so',
  'libnativewindow.so',
  'libsync.so',
  'libc.so',
]);

export function validateAndroidElf(text) {
  if (!/Class:\s+ELF64/.test(text) || !/Machine:\s+AArch64/.test(text))
    throw new Error('Android output must be AArch64 ELF64');
  const needed = [...text.matchAll(/\(NEEDED\)\s+Shared library: \[([^\]]+)\]/g)].map(
    (match) => match[1],
  );
  if (!needed.length) throw new Error('Android output has no inspectable runtime dependencies');
  for (const name of needed)
    if (!system.has(name))
      throw new Error(`Android runtime is not bundled or an approved system library: ${name}`);
  const alignments = [...text.matchAll(/^\s+LOAD\s+.+?\s+(0x[\da-f]+)\s*$/gim)].map((match) =>
    Number(match[1]),
  );
  if (!alignments.length || alignments.some((value) => value < 16384))
    throw new Error('Android load segments must support 16 KiB pages');
  return { architecture: 'AArch64 ELF64', needed, loadAlignments: alignments };
}

export function auditAndroidElf(binary, readelf) {
  const result = spawnSync(
    readelf,
    ['--file-header', '--dynamic', '--program-headers', '--wide', binary],
    { encoding: 'utf8' },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Android ELF inspection failed: ${result.stderr}`);
  return validateAndroidElf(result.stdout);
}

export function verifyAndroidApk(apk, binary, sdk, run) {
  const versions = readdirSync(join(sdk, 'build-tools'))
    .filter((name) => /^\d+\.\d+\.\d+$/.test(name))
    .sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
  if (!versions.length)
    throw new Error('Android SDK build-tools are required for package verification');
  const tools = join(sdk, 'build-tools', versions.at(-1));
  run(join(tools, process.platform === 'win32' ? 'zipalign.exe' : 'zipalign'), [
    '-c',
    '-P',
    '16',
    '4',
    apk,
  ]);
  const signer = join(tools, process.platform === 'win32' ? 'apksigner.bat' : 'apksigner');
  if (process.platform === 'win32') run('cmd.exe', ['/d', '/c', signer, 'verify', apk]);
  else run(signer, ['verify', apk]);
  const temporary = mkdtempSync(join(tmpdir(), 'hongguo-apk-native-'));
  try {
    run('jar', ['xf', apk, 'lib/arm64-v8a/libnsh.so'], {}, temporary);
    const hash = (path) => createHash('sha256').update(readFileSync(path)).digest('hex');
    if (hash(binary) !== hash(join(temporary, 'lib/arm64-v8a/libnsh.so')))
      throw new Error('APK native library differs from its captured relinking materials');
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}

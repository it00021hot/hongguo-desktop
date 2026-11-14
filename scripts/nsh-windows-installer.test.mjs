import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { buildWindowsInstaller, uninstallFiles } from './nsh-windows-installer.mjs';

test('uninstall removes only packaged paths and leaves user-added files and data directories', () => {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-installer-'));
  try {
    mkdirSync(join(directory, 'NativeMedia/licenses'), { recursive: true });
    writeFileSync(join(directory, 'Hongguo.exe'), 'application');
    writeFileSync(join(directory, 'NativeMedia/licenses/LICENSE.txt'), 'license');
    const commands = uninstallFiles(directory);
    writeFileSync(join(directory, 'user-added.mp4'), 'user video');
    assert.match(commands, /^Delete "\$INSTDIR\\Hongguo\.exe"$/m);
    assert.match(commands, /^Delete "\$INSTDIR\\NativeMedia\\licenses\\LICENSE\.txt"$/m);
    assert.ok(
      commands.indexOf('RMDir "$INSTDIR\\NativeMedia\\licenses"') <
        commands.indexOf('RMDir "$INSTDIR\\NativeMedia"'),
    );
    assert.doesNotMatch(commands, /\/r|\*|APPDATA|user-added|hongguo\.db/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test(
  'payload links cannot make the installer traverse an external directory',
  { skip: process.platform === 'win32' },
  () => {
    const directory = mkdtempSync(join(tmpdir(), 'hongguo-installer-'));
    try {
      symlinkSync(tmpdir(), join(directory, 'external'), 'dir');
      assert.throws(() => uninstallFiles(directory), /Unsupported installer payload/);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
);

test('stale FFmpeg bundles are rejected before invoking NSIS', () => {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-installer-'));
  try {
    writeFileSync(join(directory, 'Hongguo.exe'), 'stale application');
    writeFileSync(join(directory, 'avcodec-63.dll'), 'stale FFmpeg');
    assert.throws(() => buildWindowsInstaller(directory), /pinned libde265 DLL/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

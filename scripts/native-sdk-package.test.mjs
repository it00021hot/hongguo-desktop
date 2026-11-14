import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { test } from 'node:test';
import {
  archiveSdk,
  inventorySdk,
  sdkTarget,
  vvdecTarget,
  validateBuildEvidence,
} from './native-sdk-package.mjs';

const apple = () => ({
  architecture: 'arm64',
  platforms: ['1'],
  minimumVersions: ['14.0'],
  configuration: 'Release',
  sourceMatches: true,
});

test('VVdeC preserves the older shells OS baseline independently of the NSH codec SDK', () => {
  for (const [triple, version, architecture] of [
    ['aarch64-apple-darwin', '11.0', 'arm64'],
    ['x86_64-apple-darwin', '10.15', 'x86_64'],
  ]) {
    const component = vvdecTarget(triple);
    assert.equal(sdkTarget(triple).minimumOs, '14.0');
    assert.equal(component.minimumOs, version);
    assert.doesNotThrow(() =>
      validateBuildEvidence(component, {
        ...apple(),
        architecture,
        minimumVersions: [version],
      }),
    );
    assert.throws(
      () =>
        validateBuildEvidence(component, {
          ...apple(),
          architecture,
          minimumVersions: ['14.0'],
        }),
      /newer OS/,
    );
  }
});

test('native SDK export accepts the actual target ABI and rejects foreign or newer Apple objects', () => {
  const spec = sdkTarget('aarch64-apple-darwin');
  assert.doesNotThrow(() => validateBuildEvidence(spec, apple()));
  for (const change of [
    { architecture: 'x86_64' },
    { platforms: ['1', '7'] },
    { minimumVersions: ['27.2'] },
    { minimumVersions: ['14.0.1'] },
    { minimumVersions: ['invalid'] },
    { minimumVersions: [] },
    { sourceMatches: false },
  ]) {
    assert.throws(() => validateBuildEvidence(spec, { ...apple(), ...change }), /VVdeC/);
  }
});

test('Windows SDK libraries require target COFF objects and static release CRT directives', () => {
  const spec = sdkTarget('x86_64-pc-windows-msvc');
  const evidence = {
    configuration: 'Release',
    sourceMatches: true,
    architecture: 'x86_64',
    machines: ['IMAGE_FILE_MACHINE_AMD64'],
    directives: '/DEFAULTLIB:libcmt.lib /DEFAULTLIB:libcpmt.lib RuntimeLibrary=MT_StaticRelease',
  };
  assert.doesNotThrow(() => validateBuildEvidence(spec, evidence));
  for (const change of [
    { directives: '/DEFAULTLIB:MSVCRT RuntimeLibrary=MD_DynamicRelease' },
    { machines: ['IMAGE_FILE_MACHINE_AMD64', 'IMAGE_FILE_MACHINE_ARM64'] },
    { configuration: 'Debug' },
  ]) {
    assert.throws(() => validateBuildEvidence(spec, { ...evidence, ...change }), /VVdeC/);
  }
});

test('Android SDK export requires the target compiler API and matching AArch64 objects', () => {
  const spec = sdkTarget('aarch64-linux-android');
  const evidence = {
    configuration: 'Release',
    sourceMatches: true,
    architecture: 'aarch64',
    machines: ['AArch64'],
    compilerTarget: 'aarch64-none-linux-android28',
  };
  assert.doesNotThrow(() => validateBuildEvidence(spec, evidence));
  assert.throws(
    () =>
      validateBuildEvidence(spec, { ...evidence, compilerTarget: 'aarch64-none-linux-android35' }),
    /VVdeC/,
  );
});

test('SDK inventory hashes contained library aliases and refuses escaping links', () => {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-sdk-inventory-'));
  try {
    const sdk = join(directory, 'sdk');
    mkdirSync(join(sdk, 'lib'), { recursive: true });
    writeFileSync(join(directory, 'outside'), 'sentinel');
    writeFileSync(join(sdk, 'lib/actual.dylib'), 'library');
    symlinkSync('actual.dylib', join(sdk, 'lib/alias.dylib'));
    const inventory = inventorySdk(sdk);
    assert.equal(inventory.length, 2);
    assert.equal(inventory[0].sha256, inventory[1].sha256);
    symlinkSync('../../outside', join(sdk, 'lib/escape.dylib'));
    assert.throws(() => inventorySdk(sdk), /escape/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('SDK archive preserves one target root, symlinks and deterministic bytes', () => {
  const directory = mkdtempSync(join(tmpdir(), 'hongguo-sdk-archive-'));
  try {
    const triple = 'aarch64-apple-darwin';
    const target = join(directory, triple);
    mkdirSync(join(target, 'lib'), { recursive: true });
    writeFileSync(join(target, 'lib/actual.dylib'), 'library');
    symlinkSync('actual.dylib', join(target, 'lib/alias.dylib'));
    writeFileSync(join(target, 'sdk.json'), '{}\n');
    const first = join(directory, 'first.tar.gz');
    const second = join(directory, 'second.tar.gz');
    archiveSdk(target, first);
    archiveSdk(target, second);
    assert.deepEqual(readFileSync(first), readFileSync(second));
    const listing = execFileSync('tar', ['-tzf', first], { encoding: 'utf8' }).trim().split('\n');
    assert.ok(listing.every((entry) => entry.startsWith(`${triple}/`)));
    const verbose = execFileSync('tar', ['-tvzf', first], { encoding: 'utf8' });
    assert.match(verbose, /alias\.dylib -> actual\.dylib/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

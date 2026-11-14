import assert from 'node:assert/strict';
import { test } from 'node:test';
import { validateAndroidElf } from './nsh-android-audit.mjs';

const elf = (library = 'libc.so', alignment = '0x4000') =>
  `Class: ELF64\nMachine: AArch64\n  LOAD 0x0 0x0 0x0 0x2000 0x2000 R E ${alignment}\n  0x1 (NEEDED) Shared library: [${library}]\n`;

test('Android delivery rejects a shared C++ runtime, foreign architecture and 4 KiB-only segments', () => {
  assert.equal(validateAndroidElf(elf()).architecture, 'AArch64 ELF64');
  for (const library of ['libc++_shared.so', 'libffmpeg.so', 'libgcc_s.so'])
    assert.throws(() => validateAndroidElf(elf(library)), /runtime is not bundled/);
  assert.throws(() => validateAndroidElf(elf().replace('AArch64', 'X86-64')), /AArch64/);
  assert.throws(() => validateAndroidElf(elf('libc.so', '0x1000')), /16 KiB/);
});

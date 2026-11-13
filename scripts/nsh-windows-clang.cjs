#!/usr/bin/env node
const { spawnSync } = require('node:child_process');

const compiler = process.env.NSH_WINDOWS_CLANG;
if (!compiler) throw new Error('Set NSH_WINDOWS_CLANG to the LLVM 19 clang-cl executable');

const driverWarnings = new Set(['-Werror', '-Wno-unused-command-line-argument']);
const args = process.argv.slice(3).map((argument) => {
  const isDriverWarning = driverWarnings.has(argument) || /^-W[0-4]$/.test(argument);
  return argument === '-pedantic' || (argument.startsWith('-W') && !isDriverWarning)
    ? `/clang:${argument}`
    : argument;
});
const result = spawnSync(compiler, args, { stdio: 'inherit' });
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;

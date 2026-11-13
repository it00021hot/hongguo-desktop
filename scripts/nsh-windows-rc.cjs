#!/usr/bin/env node
const { spawnSync } = require('node:child_process');

const compiler = process.env.NSH_WINDOWS_RC;
if (!compiler) throw new Error('Set NSH_WINDOWS_RC to the llvm-rc executable');

const input = process.argv.slice(2);
const args = [];
for (let index = 0; index < input.length; index += 1) {
  const argument = input[index];
  const option = argument.toLowerCase();
  if (option === '/fo') {
    const output = input[++index];
    if (!output) throw new Error('Missing resource output path after /fo');
    args.push('-fo', output);
  } else if (option.startsWith('/fo')) {
    args.push('-fo', argument.slice(3));
  } else if (option === '/nologo') {
    args.push('-nologo');
  } else {
    args.push('--', argument);
  }
}
const result = spawnSync(compiler, args, { stdio: 'inherit' });
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;

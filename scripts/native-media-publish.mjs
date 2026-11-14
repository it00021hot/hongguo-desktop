import { createHash } from 'node:crypto';
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export function publishPrefix(staging, destination, run) {
  if (!existsSync(destination)) {
    renameSync(staging, destination);
    return;
  }
  if (process.platform === 'darwin') {
    const source = fileURLToPath(new URL('./native-media-publish.c', import.meta.url));
    const digest = createHash('sha256').update(readFileSync(source)).digest('hex').slice(0, 12);
    const helper = join(dirname(destination), `publish-${digest}`);
    if (!existsSync(helper)) {
      const compiler = run('xcrun', ['--sdk', 'macosx', '--find', 'clang']).trim();
      const sdk = run('xcrun', ['--sdk', 'macosx', '--show-sdk-path']).trim();
      run(compiler, ['-isysroot', sdk, source, '-o', helper]);
    }
    run(helper, [staging, destination]);
    return;
  }
  const previous = `${staging}.previous`;
  renameSync(destination, previous);
  try {
    renameSync(staging, destination);
  } catch (error) {
    renameSync(previous, destination);
    throw error;
  }
}

export function rewritePkgconfig(staging, destination) {
  const directory = join(staging, 'lib/pkgconfig');
  mkdirSync(directory, { recursive: true });
  for (const name of readdirSync(directory)) {
    const path = join(directory, name);
    writeFileSync(path, readFileSync(path, 'utf8').replaceAll(staging, destination));
  }
}

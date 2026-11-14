import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const libraries = ['avcodec', 'avutil', 'swresample', 'avfilter'];
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');

function childPath(parent, path) {
  const child = relative(parent, path);
  return child === '..' || child.startsWith(`..${sep}`) || isAbsolute(child)
    ? null
    : child.replaceAll('\\', '/');
}

export function responseArguments(text) {
  const values = [];
  let value = '',
    quote = '',
    escaped = false,
    present = false;
  for (const character of text) {
    if (escaped) {
      value += character;
      escaped = false;
      present = true;
    } else if (character === '\\' && quote !== "'") escaped = true;
    else if (quote) {
      if (character === quote) quote = '';
      else value += character;
      present = true;
    } else if (character === '"' || character === "'") {
      quote = character;
      present = true;
    } else if (/\s/.test(character)) {
      if (present) {
        values.push(value);
        value = '';
        present = false;
      }
    } else {
      value += character;
      present = true;
    }
  }
  if (quote || escaped) throw new Error('Invalid linker response file quoting');
  if (present) values.push(value);
  return values;
}

function expandResponse(args, depth = 0) {
  if (depth > 8) throw new Error('Linker response nesting exceeds eight levels');
  return args.flatMap((argument) =>
    argument.startsWith('@') && existsSync(argument.slice(1))
      ? expandResponse(responseArguments(readFileSync(argument.slice(1), 'utf8')), depth + 1)
      : [argument],
  );
}

function execute(command, args, options = {}) {
  let response;
  try {
    if (process.platform === 'win32' || args.join(' ').length > 16000) {
      response = mkdtempSync(join(tmpdir(), 'hongguo-link-arguments-'));
      const path = join(response, 'link.args');
      writeFileSync(
        path,
        args
          .map((argument) =>
            JSON.stringify(
              process.platform === 'win32' ? argument.replaceAll('\\', '/') : argument,
            ),
          )
          .join('\n'),
      );
      args = [`@${path}`];
    }
    const result = spawnSync(command, args, { stdio: 'inherit', ...options });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(`${command} exited with ${result.status}`);
  } finally {
    if (response) rmSync(response, { recursive: true, force: true });
  }
}

export function captureLink(args, env = process.env) {
  const linker = env.HONGGUO_RELINK_REAL_LINKER;
  const destination = env.HONGGUO_RELINK_OUTPUT;
  const sdk = env.HONGGUO_RELINK_SDK_ROOT;
  const prefix = env.HONGGUO_FFMPEG_PREFIX;
  if (![linker, destination, sdk, prefix, env.HONGGUO_RELINK_TARGET].every(Boolean))
    throw new Error(
      'Relink capture requires a target, output, real linker, target SDK and FFmpeg prefix',
    );
  args = [...JSON.parse(env.HONGGUO_RELINK_REAL_LINKER_ARGS ?? '[]'), ...expandResponse(args)];
  const outputIndex = args.indexOf('-o') + 1;
  if (!outputIndex || !args[outputIndex])
    throw new Error('Final linker invocation has no -o output');
  mkdirSync(dirname(destination), { recursive: true });
  const staging = mkdtempSync(`${destination}.staging-`);
  const objects = join(staging, 'objects');
  mkdirSync(objects);
  const files = [];
  const captured = new Map();
  const save = (path, name) => {
    const bytes = readFileSync(path);
    const sha256 = digest(bytes);
    const local = name ?? `objects/${sha256}-${basename(path)}`;
    if (!captured.has(local)) {
      mkdirSync(dirname(join(staging, local)), { recursive: true });
      copyFileSync(path, join(staging, local));
      files.push({ path: local, sha256, byteLength: bytes.length });
      captured.set(local, true);
    }
    return `{MATERIALS}/${local}`;
  };
  const sdkPath = resolve(sdk);
  const prefixPath = resolve(prefix);
  const toolchainPath = env.HONGGUO_RELINK_TARGET.includes('apple')
    ? resolve(dirname(dirname(linker)))
    : null;
  const rewritePath = (path, search = false) => {
    const absolute = resolve(path);
    for (const [parent, token] of [
      [sdkPath, '{SDKROOT}'],
      [prefixPath, '{FFMPEG_PREFIX}'],
      [toolchainPath, '{TOOLCHAIN_ROOT}'],
    ]) {
      if (!parent) continue;
      const child = childPath(parent, absolute);
      if (child !== null) return token + (child ? `/${child}` : '');
    }
    if (!existsSync(absolute)) return path;
    if (statSync(absolute).isFile()) return save(absolute);
    if (search) {
      const directory = `search/${digest(Buffer.from(absolute)).slice(0, 16)}`;
      mkdirSync(join(staging, directory), { recursive: true });
      for (const name of readdirSync(absolute)) {
        if (/\.(?:a|so|dylib)$/.test(name) && statSync(join(absolute, name)).isFile())
          save(join(absolute, name), `${directory}/${name}`);
      }
      return `{MATERIALS}/${directory}`;
    }
    return path;
  };
  const rewrite = (value, search = false) => {
    if (isAbsolute(value) || existsSync(value)) return rewritePath(value, search);
    if (value.startsWith('-Wl,'))
      return (
        '-Wl,' +
        value
          .slice(4)
          .split(',')
          .map((piece) => rewrite(piece))
          .join(',')
      );
    const equals = value.indexOf('=');
    if (equals >= 0 && isAbsolute(value.slice(equals + 1)))
      return value.slice(0, equals + 1) + rewritePath(value.slice(equals + 1), search);
    if (/^-[LF]/.test(value) && isAbsolute(value.slice(2)))
      return value.slice(0, 2) + rewritePath(value.slice(2), true);
    return value;
  };
  try {
    for (const library of libraries)
      save(join(prefixPath, 'lib', `lib${library}.a`), `ffmpeg/lib/lib${library}.a`);
    const command = args.map((argument, index) =>
      index === outputIndex
        ? '{OUTPUT}'
        : rewrite(argument, ['-L', '-F'].includes(args[index - 1])),
    );
    if (!files.some((file) => file.path.startsWith('objects/')))
      throw new Error('Final link captured no application objects');
    execute(linker, args);
    const manifest = {
      schemaVersion: 1,
      target: env.HONGGUO_RELINK_TARGET,
      linker,
      sdk,
      outputName: basename(args[outputIndex]),
      linkedOutputPath: resolve(args[outputIndex]),
      outputSha256: digest(readFileSync(args[outputIndex])),
      args: command,
      files,
    };
    writeFileSync(join(staging, 'link.json'), JSON.stringify(manifest, null, 2));
    copyFileSync(fileURLToPath(import.meta.url), join(staging, 'relink.mjs'));
    const previous = `${destination}.previous`;
    rmSync(previous, { recursive: true, force: true });
    if (existsSync(destination)) renameSync(destination, previous);
    renameSync(staging, destination);
    rmSync(previous, { recursive: true, force: true });
  } catch (error) {
    rmSync(staging, { recursive: true, force: true });
    throw error;
  }
}

export function replayLink(materials, { linker, sdk, prefix, output }) {
  const manifest = JSON.parse(readFileSync(join(materials, 'link.json'), 'utf8'));
  if (manifest.schemaVersion !== 1 || !manifest.files.length)
    throw new Error('Invalid relink manifest');
  for (const file of manifest.files) {
    const path = resolve(materials, file.path);
    if (
      childPath(resolve(materials), path) === null ||
      statSync(path).size !== file.byteLength ||
      digest(readFileSync(path)) !== file.sha256
    )
      throw new Error(`Relink material is missing or modified: ${file.path}`);
  }
  if (!linker || !sdk || !output) throw new Error('Relink requires --linker, --sdk and --output');
  const substitutions = {
    '{MATERIALS}': resolve(materials),
    '{SDKROOT}': resolve(sdk),
    '{TOOLCHAIN_ROOT}': resolve(dirname(dirname(linker))),
    '{FFMPEG_PREFIX}': resolve(prefix ?? join(materials, 'ffmpeg')),
    '{OUTPUT}': resolve(output),
  };
  const args = manifest.args.map((value) => {
    for (const [token, path] of Object.entries(substitutions))
      value = value.replaceAll(token, path);
    return value;
  });
  mkdirSync(dirname(resolve(output)), { recursive: true });
  execute(linker, args, {
    env: { ...process.env, ...(manifest.target.includes('apple') ? { SDKROOT: sdk } : {}) },
  });
  return manifest;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const options = {};
    const args = process.argv.slice(2);
    while (args.length) {
      const key = args.shift();
      const value = args.shift();
      if (!['--linker', '--sdk', '--ffmpeg', '--output'].includes(key) || !value)
        throw new Error(
          'Usage: node relink.mjs --linker <target compiler> --sdk <SDK/NDK root> [--ffmpeg <replacement prefix>] --output <binary>',
        );
      options[key === '--ffmpeg' ? 'prefix' : key.slice(2)] = value;
    }
    replayLink(dirname(fileURLToPath(import.meta.url)), options);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

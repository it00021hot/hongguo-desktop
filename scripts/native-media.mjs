import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  appendFileSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  writeFileSync,
} from 'node:fs';
import { availableParallelism } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { configuration, resolveToolchain, source, targetSpec } from './native-media-config.mjs';
import { prepareSource, sourcePatches } from './native-media-source.mjs';
import { inspectLibraries, verifyInstall, writeManifest } from './native-media-manifest.mjs';
import { publishPrefix, rewritePkgconfig } from './native-media-publish.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: options.cwd ?? root,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
    env: { ...process.env, ...options.env },
  });
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`;
  if (options.log) appendFileSync(options.log, output);
  if (result.error) throw new Error(`${command}: ${result.error.message}`, { cause: result.error });
  if (result.status !== 0 && !options.acceptNonzero) {
    throw new Error(
      `${command} exited with ${result.status}${options.log ? ` (${options.log})` : ''}\n${output.slice(-8192)}`,
    );
  }
  return output;
}

function layout(spec) {
  const directory = join(root, 'target/native-media', spec.triple);
  return {
    directory,
    prefix: join(directory, `ffmpeg-${source.version}`),
    work: join(directory, `build-ffmpeg-${source.version}`),
  };
}

function build(spec) {
  const paths = layout(spec);
  const toolchain = resolveToolchain(spec, run);
  const prepared = prepareSource(
    join(root, 'target/native-media/sources'),
    run,
    join(paths.directory, `source-ffmpeg-${source.version}`),
    sourcePatches(spec.os),
  );
  mkdirSync(paths.work, { recursive: true });
  const log = join(paths.directory, 'build.log');
  const staging = mkdtempSync(join(paths.directory, `.ffmpeg-${source.version}-`));
  const stagedPaths = { ...paths, prefix: staging };
  const args = configuration(spec, paths.prefix, toolchain);
  const env = {
    SOURCE_DATE_EPOCH: String(source.sourceDateEpoch),
    PKG_CONFIG: 'false',
    PKG_CONFIG_PATH: '',
    PKG_CONFIG_LIBDIR: '',
    CPATH: '',
    C_INCLUDE_PATH: '',
    CPLUS_INCLUDE_PATH: '',
    LIBRARY_PATH: '',
    CFLAGS: '',
    CXXFLAGS: '',
    CPPFLAGS: '',
    LDFLAGS: '',
    ...toolchain.env,
  };
  const jobs = Number(process.env.HONGGUO_NATIVE_MEDIA_JOBS ?? Math.min(4, availableParallelism()));
  if (!Number.isInteger(jobs) || jobs < 1 || jobs > 64)
    throw new Error('HONGGUO_NATIVE_MEDIA_JOBS must be an integer from 1 to 64');
  writeFileSync(log, `FFmpeg ${source.version} ${spec.triple}\n${JSON.stringify(args)}\n`);
  const options = { cwd: paths.work, env, log };
  console.log(`Configuring FFmpeg ${source.version}: ${spec.triple}`);
  run(
    toolchain.shell,
    [join(prepared.directory, 'configure').replaceAll('\\', '/'), ...args],
    options,
  );
  validateConfiguration(paths.work, spec);
  console.log(`Building FFmpeg with ${jobs} jobs; log: ${log}`);
  run(toolchain.make, [`-j${jobs}`], options);
  run(toolchain.make, ['install', `prefix=${staging.replaceAll('\\', '/')}`], options);
  if (spec.os === 'windows') {
    mkdirSync(join(staging, 'lib'), { recursive: true });
    for (const name of readdirSync(join(staging, 'bin'))) {
      if (name.endsWith('.lib')) renameSync(join(staging, 'bin', name), join(staging, 'lib', name));
    }
  }
  copyBuildInputs(stagedPaths, prepared, args);
  rewritePkgconfig(staging, paths.prefix);
  const libraryInspection = inspectLibraries(staging, spec, run, toolchain);
  const patches = prepared.patches.map((path) => ({
    file: `sources/${path.split(/[\\/]/).at(-1)}`,
    sha256: createHash('sha256').update(readFileSync(path)).digest('hex'),
  }));
  const manifest = writeManifest(staging, {
    source,
    patches,
    target: spec,
    toolchain,
    configureArgs: args,
    libraryInspection,
  });
  verifyInstall(staging, spec);
  publishPrefix(staging, paths.prefix, run);
  verifyInstall(paths.prefix, spec);
  console.log(`HONGGUO_FFMPEG_PREFIX=${paths.prefix}`);
  console.log(`Verified ${manifest.artifacts.length} installed artifacts`);
}

function validateConfiguration(work, spec) {
  const text = readFileSync(join(work, 'config_components.h'), 'utf8');
  for (const component of [
    'H264_DECODER',
    'HEVC_DECODER',
    'AAC_DECODER',
    'H264_PARSER',
    'HEVC_PARSER',
    'AAC_PARSER',
    'ATEMPO_FILTER',
    'ARESAMPLE_FILTER',
    'AFORMAT_FILTER',
    'ANULL_FILTER',
  ]) {
    if (!text.includes(`#define CONFIG_${component} 1`))
      throw new Error(`Required FFmpeg component disabled: ${component}`);
  }
  if (/^#define CONFIG_\w+_ENCODER 1$/m.test(text))
    throw new Error('FFmpeg build unexpectedly enabled an encoder');
  const config = readFileSync(join(work, 'config.h'), 'utf8');
  if (!config.includes('#define CONFIG_GPL 0') || !config.includes('#define CONFIG_NONFREE 0'))
    throw new Error('FFmpeg build is not the required LGPL configuration');
  if (!config.includes('#define CONFIG_MEDIAFOUNDATION 0'))
    throw new Error('FFmpeg build unexpectedly depends on Media Foundation');
  const hardware =
    spec.os === 'windows' ? 'D3D11VA' : spec.os === 'android' ? 'MEDIACODEC' : 'VIDEOTOOLBOX';
  if (!config.includes(`#define CONFIG_${hardware} 1`))
    throw new Error(`Required hardware API disabled: ${hardware}`);
  if (spec.os === 'windows') {
    for (const codec of ['H264', 'HEVC']) {
      if (!text.includes(`#define CONFIG_${codec}_D3D11VA2_HWACCEL 1`))
        throw new Error(`Required D3D11 texture decoder disabled: ${codec}`);
      if (!text.includes(`#define CONFIG_${codec}_D3D12VA_HWACCEL 1`))
        throw new Error(`Required D3D12 audit decoder disabled: ${codec}`);
    }
  }
}

function copyBuildInputs(paths, prepared, args) {
  const licenses = join(paths.prefix, 'licenses');
  const sources = join(paths.prefix, 'sources');
  const configuration = join(paths.prefix, 'build-info');
  for (const directory of [licenses, sources, configuration])
    mkdirSync(directory, { recursive: true });
  for (const name of ['COPYING.LGPLv2.1', 'LICENSE.md'])
    copyFileSync(join(prepared.directory, name), join(licenses, name));
  for (const [name, path] of Object.entries(prepared)) {
    if (name !== 'directory' && name !== 'patches')
      copyFileSync(path, join(sources, path.split(/[\\/]/).at(-1)));
  }
  for (const path of prepared.patches)
    copyFileSync(path, join(sources, path.split(/[\\/]/).at(-1)));
  for (const name of ['config.h', 'config_components.h', 'ffbuild/config.mak']) {
    copyFileSync(join(paths.work, name), join(configuration, name.replaceAll('/', '-')));
  }
  writeFileSync(join(configuration, 'configure.json'), `${JSON.stringify(args, null, 2)}\n`);
  writeFileSync(
    join(licenses, 'NOTICE.txt'),
    `This application uses FFmpeg ${source.version} under LGPL-2.1-or-later.\nCorresponding upstream source, release signature and local changes: ../sources/\nConfiguration: ../build-info/\nhttps://ffmpeg.org/legal.html\n`,
  );
}

function probe(spec) {
  const { prefix, directory } = layout(spec);
  const manifest = verifyInstall(prefix, spec);
  const hostArch = process.arch === 'arm64' ? 'aarch64' : process.arch;
  if (
    spec.os !== { darwin: 'macos', win32: 'windows' }[process.platform] ||
    spec.arch !== hostArch
  ) {
    throw new Error('Native media probe executes only on its matching desktop host');
  }
  const program = join(directory, process.platform === 'win32' ? 'probe.exe' : 'probe');
  const source = join(root, 'scripts/native-media-probe.c');
  const args =
    spec.os === 'windows'
      ? [
          '/nologo',
          '/MT',
          `/I${join(prefix, 'include')}`,
          source,
          `/Fe:${program}`,
          '/link',
          `/LIBPATH:${join(prefix, 'lib')}`,
          'avcodec.lib',
          'avutil.lib',
          'swresample.lib',
          'avfilter.lib',
        ]
      : [
          source,
          '-isysroot',
          manifest.toolchain.sdk,
          '-I',
          join(prefix, 'include'),
          '-L',
          join(prefix, 'lib'),
          '-lavcodec',
          '-lavutil',
          '-lswresample',
          '-lavfilter',
          `-Wl,-rpath,${join(prefix, 'lib')}`,
          '-o',
          program,
        ];
  run(manifest.toolchain.compiler, args);
  console.log(
    run(program, [], {
      env:
        spec.os === 'windows' ? { PATH: `${join(prefix, 'bin')};${process.env.PATH ?? ''}` } : {},
    }).trim(),
  );
}

function main() {
  const [action, triple, ...extra] = process.argv.slice(2);
  if (!['build', 'plan', 'verify', 'probe'].includes(action) || extra.length)
    throw new Error(
      'Usage: node scripts/native-media.mjs build|plan|verify|probe <Rust target triple>',
    );
  const spec = targetSpec(triple);
  if (action === 'build') return build(spec);
  if (action === 'probe') return probe(spec);
  const { prefix } = layout(spec);
  if (action === 'verify') {
    const manifest = verifyInstall(prefix, spec);
    console.log(`Verified ${manifest.artifacts.length} artifacts: ${prefix}`);
    return;
  }
  const toolchain = resolveToolchain(spec, run);
  console.log(
    JSON.stringify(
      { prefix, target: spec, toolchain, configureArgs: configuration(spec, prefix, toolchain) },
      null,
      2,
    ),
  );
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

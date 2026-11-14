import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { windowsToolchain } from './native-media-windows.mjs';

export const { ffmpeg: source } = JSON.parse(
  readFileSync(new URL('./native-media.sources.json', import.meta.url), 'utf8'),
);
export const libraries = ['avcodec', 'avutil', 'swresample', 'avfilter'];

const targets = {
  'aarch64-apple-darwin': { os: 'macos', arch: 'aarch64', sdk: 'macosx', minimumOs: '14.0' },
  'x86_64-apple-darwin': { os: 'macos', arch: 'x86_64', sdk: 'macosx', minimumOs: '14.0' },
  'x86_64-pc-windows-msvc': { os: 'windows', arch: 'x86_64', minimumOs: '10.0' },
  'aarch64-apple-ios': { os: 'ios', arch: 'aarch64', sdk: 'iphoneos', minimumOs: '16.0' },
  'aarch64-apple-ios-sim': {
    os: 'ios',
    arch: 'aarch64',
    sdk: 'iphonesimulator',
    minimumOs: '16.0',
  },
  'aarch64-linux-android': { os: 'android', arch: 'aarch64', minimumOs: '28' },
};

export function targetSpec(triple) {
  const target = targets[triple];
  if (!target) throw new Error(`Unsupported native media target: ${triple}`);
  return {
    ...target,
    triple,
    linkage: ['macos', 'windows'].includes(target.os) ? 'shared' : 'static',
  };
}

export function configuration(spec, prefix, toolchain) {
  return [
    `--prefix=${prefix.replaceAll('\\', '/')}`,
    '--disable-everything',
    '--disable-autodetect',
    '--disable-doc',
    '--disable-programs',
    '--disable-network',
    '--disable-mediafoundation',
    '--disable-avdevice',
    '--disable-avformat',
    '--disable-swscale',
    '--disable-encoders',
    '--disable-gpl',
    '--disable-nonfree',
    '--disable-debug',
    '--pkg-config=false',
    '--enable-pic',
    '--enable-runtime-cpudetect',
    ...libraries.map((library) => `--enable-${library}`),
    '--enable-decoder=h264,hevc,aac',
    '--enable-parser=h264,hevc,aac',
    '--enable-filter=abuffer,abuffersink,atempo,aformat,aresample,anull',
    `--enable-${spec.linkage}`,
    `--disable-${spec.linkage === 'shared' ? 'static' : 'shared'}`,
    ...toolchain.args,
  ];
}

export function resolveToolchain(spec, run, env = process.env, host = process.platform) {
  if (['macos', 'ios'].includes(spec.os)) return appleToolchain(spec, run, host);
  if (spec.os === 'android') return androidToolchain(spec, run, env, host);
  return windowsToolchain(run, env, host);
}

function appleToolchain(spec, run, host) {
  if (host !== 'darwin') throw new Error('Apple FFmpeg requires macOS and the target Apple SDK');
  const sdk = run('xcrun', ['--sdk', spec.sdk, '--show-sdk-path']).trim();
  const compiler = run('xcrun', ['--sdk', spec.sdk, '--find', 'clang']).trim();
  const arch = spec.arch === 'aarch64' ? 'arm64' : 'x86_64';
  const triple =
    spec.os === 'macos'
      ? `${arch}-apple-macos${spec.minimumOs}`
      : `${arch}-apple-ios${spec.minimumOs}${spec.sdk === 'iphonesimulator' ? '-simulator' : ''}`;
  return {
    compiler,
    sdk,
    make: 'make',
    shell: 'sh',
    args: [
      '--target-os=darwin',
      `--arch=${spec.arch}`,
      '--cpu=generic',
      '--enable-cross-compile',
      `--cc=${compiler}`,
      `--sysroot=${sdk}`,
      `--extra-cflags=-target ${triple}`,
      `--extra-ldflags=-target ${triple}`,
      '--enable-videotoolbox',
      '--enable-hwaccel=h264_videotoolbox,hevc_videotoolbox',
      ...(spec.linkage === 'shared' ? ['--install-name-dir=@rpath'] : []),
    ],
    version: run(compiler, ['--version']).trim(),
  };
}

function androidToolchain(spec, run, env, host) {
  const ndk = env.NDK_HOME;
  if (!ndk || !existsSync(ndk)) throw new Error('NDK_HOME must name the Android NDK');
  const tag = { darwin: 'darwin-x86_64', linux: 'linux-x86_64', win32: 'windows-x86_64' }[host];
  if (!tag) throw new Error(`Unsupported Android build host: ${host}`);
  const bin = join(ndk, 'toolchains/llvm/prebuilt', tag, 'bin');
  const executable = (name) =>
    join(bin, `${name}${host === 'win32' ? '.exe' : ''}`).replaceAll('\\', '/');
  const compiler =
    host === 'win32'
      ? executable('clang')
      : join(bin, `aarch64-linux-android${spec.minimumOs}-clang`);
  const compilerTarget =
    host === 'win32' ? [`--target=aarch64-linux-android${spec.minimumOs}`] : [];
  if (!existsSync(compiler)) throw new Error(`Android target compiler is required: ${compiler}`);
  return {
    compiler,
    sdk: ndk,
    make: 'make',
    shell: host === 'win32' ? 'bash' : 'sh',
    args: [
      '--target-os=android',
      '--arch=aarch64',
      '--cpu=generic',
      '--enable-cross-compile',
      `--cc=${compiler}`,
      `--ar=${executable('llvm-ar')}`,
      `--nm=${executable('llvm-nm')}`,
      `--ranlib=${executable('llvm-ranlib')}`,
      `--strip=${executable('llvm-strip')}`,
      ...compilerTarget.flatMap((target) => [
        `--extra-cflags=${target}`,
        `--extra-ldflags=${target}`,
      ]),
      '--enable-jni',
      '--enable-mediacodec',
      '--enable-decoder=h264_mediacodec,hevc_mediacodec',
    ],
    version: run(compiler, [...compilerTarget, '--version']).trim(),
  };
}

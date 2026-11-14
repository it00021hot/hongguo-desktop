import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, symlinkSync } from 'node:fs';
import { delimiter, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const hardware =
  '--enable-hwaccel=h264_d3d11va,hevc_d3d11va,h264_d3d11va2,hevc_d3d11va2,h264_d3d12va,hevc_d3d12va';

export function windowsToolchain(run, env, host) {
  if (host === 'win32') {
    return {
      compiler: 'cl',
      make: 'make',
      shell: 'bash',
      args: [
        '--toolchain=msvc',
        '--target-os=win32',
        '--arch=x86_64',
        '--cpu=generic',
        '--cc=cl',
        '--extra-cflags=-MT -D_WIN32_WINNT=0x0A00',
        '--enable-d3d11va',
        '--enable-d3d12va',
        hardware,
      ],
      version: run('cl', [], { acceptNonzero: true }).trim(),
    };
  }
  if (host !== 'darwin') throw new Error('Windows FFmpeg requires Windows MSVC or macOS xwin');
  const cache = env.XWIN_CACHE_DIR;
  const llvm = env.HONGGUO_WINDOWS_LLVM_BIN;
  const linker = env.HONGGUO_WINDOWS_LINKER;
  if (!cache || !llvm || !linker)
    throw new Error('Set XWIN_CACHE_DIR, HONGGUO_WINDOWS_LLVM_BIN and HONGGUO_WINDOWS_LINKER');
  const sdk = join(cache, 'xwin');
  const compiler = join(llvm, 'clang-cl');
  const ar = join(llvm, 'llvm-lib');
  const nm = join(llvm, 'llvm-nm');
  const readobj = join(llvm, 'llvm-readobj');
  const includes = [
    'crt/include',
    'sdk/include/ucrt',
    'sdk/include/um',
    'sdk/include/shared',
    'sdk/include/winrt',
  ].map((path) => join(sdk, path));
  const libraries = ['crt/lib/x86_64', 'sdk/lib/ucrt/x86_64', 'sdk/lib/um/x86_64'].map((path) =>
    join(sdk, path),
  );
  for (const path of [compiler, ar, nm, readobj, linker, ...includes, ...libraries]) {
    if (!existsSync(path)) throw new Error(`Windows target toolchain is missing: ${path}`);
    if (/\s/.test(path))
      throw new Error(`FFmpeg MSVC cross-toolchain paths cannot contain whitespace: ${path}`);
  }
  const tools = fileURLToPath(new URL('../target/native-media/windows-tools/', import.meta.url));
  mkdirSync(tools, { recursive: true });
  const lib = join(tools, 'lib.exe');
  if (!existsSync(lib)) symlinkSync(resolve(ar), lib);
  const packages = readFileSync(join(sdk, 'DONE'), 'utf8');
  return {
    compiler,
    sdk,
    make: 'make',
    shell: 'sh',
    readobj,
    linker,
    env: { PATH: `${tools}${delimiter}${env.PATH ?? ''}`, NM: nm },
    args: [
      '--toolchain=msvc',
      '--target-os=win32',
      '--arch=x86_64',
      '--cpu=generic',
      '--enable-cross-compile',
      `--cc=${compiler}`,
      `--ld=${linker}`,
      `--ar=${ar}`,
      `--nm=${nm}`,
      `--extra-cflags=--target=x86_64-pc-windows-msvc -MT -D_WIN32_WINNT=0x0A00 ${includes.map((path) => `-imsvc${path}`).join(' ')}`,
      `--extra-ldflags=-machine:x64 ${libraries.map((path) => `-libpath:${path}`).join(' ')}`,
      '--enable-d3d11va',
      '--enable-d3d12va',
      hardware,
    ],
    version: run(compiler, ['--version']).trim(),
    linkerVersion: run(linker, ['--version']).trim(),
    targetSdk: {
      packages: packages.trim().split('\n'),
      packageInventorySha256: createHash('sha256').update(packages).digest('hex'),
    },
  };
}

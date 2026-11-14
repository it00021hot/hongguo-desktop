import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { delimiter, dirname, join, resolve, win32 } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const hevcSource = Object.freeze({
  version: '1.1.3',
  commit: 'ba62bf4cfb3242f3bf0a45617ff09e35236e4d82',
  url: 'https://codeload.github.com/strukturag/libde265/tar.gz/ba62bf4cfb3242f3bf0a45617ff09e35236e4d82',
  sha256: '7ffe3220bba8282cc6d4a2bcf36ac1b6d2899f402c335f7df377317414a025b1',
});

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function compilerIdentity(build) {
  const files = join(build, 'CMakeFiles');
  const version = readdirSync(files).find((name) =>
    existsSync(join(files, name, 'CMakeCXXCompiler.cmake')),
  );
  if (!version) throw new Error('CMake did not record its target C++ compiler');
  const data = readFileSync(join(files, version, 'CMakeCXXCompiler.cmake'), 'utf8');
  const values = ['CMAKE_CXX_COMPILER', 'CMAKE_CXX_COMPILER_ID', 'CMAKE_CXX_COMPILER_VERSION'].map(
    (key) => {
      const value = new RegExp('set\\(' + key + ' "([^"]+)"\\)').exec(data)?.[1];
      if (!value) throw new Error('Missing CMake compiler identity: ' + key);
      return key + '=' + value;
    },
  );
  return values.join('\n');
}
export const codecTargets = Object.freeze({
  'aarch64-apple-darwin': { os: 'macos', arch: 'arm64', minimumOs: '14.0', shared: true },
  'x86_64-apple-darwin': { os: 'macos', arch: 'x86_64', minimumOs: '14.0', shared: true },
  'x86_64-pc-windows-msvc': {
    os: 'windows',
    arch: 'x86_64',
    minimumOs: '10.0.17763',
    shared: true,
  },
  'aarch64-linux-android': { os: 'android', arch: 'aarch64', minimumOs: 28, shared: false },
  'aarch64-apple-ios': { os: 'ios', arch: 'arm64', minimumOs: '16.0', shared: false },
  'aarch64-apple-ios-sim': { os: 'ios-simulator', arch: 'arm64', minimumOs: '16.0', shared: false },
});

export function codecTarget(triple) {
  const spec = codecTargets[triple];
  if (!spec) throw new Error('Unsupported native codec target: ' + triple);
  return { triple, ...spec, crt: spec.os === 'windows' ? 'static' : 'system' };
}

function run(command, args, options = {}) {
  if (process.platform === 'win32' && ['curl', 'tar'].includes(command))
    command = win32.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', command + '.exe');
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
    stdio: options.capture ? 'pipe' : 'inherit',
    env: options.env ?? process.env,
  });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(command + ' failed (' + result.status + '): ' + (result.stderr ?? ''));
  return result.stdout ?? '';
}

function windowsOptions(env) {
  if (process.platform === 'win32')
    return ['-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded', '-DCMAKE_POLICY_DEFAULT_CMP0091=NEW'];
  if (process.platform !== 'darwin')
    throw new Error('Windows source rebuild requires MSVC or macOS xwin');
  const sdk = join(
    env.XWIN_CACHE_DIR ?? join(process.env.HOME, 'Library/Caches/hongguo-nsh-xwin-symbols'),
    'xwin',
  );
  const llvm = env.HONGGUO_WINDOWS_LLVM_BIN ?? '/opt/homebrew/opt/llvm@19/bin';
  const linker =
    env.HONGGUO_WINDOWS_LINKER ??
    (env.PATH ?? '')
      .split(delimiter)
      .map((directory) => join(directory, 'lld-link'))
      .find(existsSync);
  if (!linker) throw new Error('Set HONGGUO_WINDOWS_LINKER or provide lld-link on PATH');
  const include = [
    'crt/include',
    'sdk/include/ucrt',
    'sdk/include/um',
    'sdk/include/shared',
    'sdk/include/winrt',
  ].map((name) => join(sdk, name));
  const libraries = ['crt/lib/x86_64', 'sdk/lib/ucrt/x86_64', 'sdk/lib/um/x86_64'].map((name) =>
    join(sdk, name),
  );
  for (const path of [join(llvm, 'clang-cl'), linker, ...include, ...libraries])
    if (!existsSync(path)) throw new Error('Windows rebuild toolchain missing: ' + path);
  env.NSH_WINDOWS_CLANG = join(llvm, 'clang-cl');
  env.NSH_WINDOWS_RC = env.HONGGUO_WINDOWS_RC ?? '/opt/homebrew/opt/llvm/bin/llvm-rc';
  const flags =
    '--target=x86_64-pc-windows-msvc ' + include.map((path) => '-imsvc"' + path + '"').join(' ');
  const linkFlags = libraries.map((path) => '/LIBPATH:"' + path + '"').join(' ');
  return [
    '-DCMAKE_SYSTEM_NAME=Windows',
    '-DCMAKE_SYSTEM_PROCESSOR=AMD64',
    '-DCMAKE_C_COMPILER=' + join(llvm, 'clang-cl'),
    '-DCMAKE_CXX_COMPILER=' + join(llvm, 'clang-cl'),
    '-DCMAKE_AR=' + join(llvm, 'llvm-lib'),
    '-DCMAKE_LINKER=' + linker,
    '-DCMAKE_MT=' + join(llvm, 'llvm-mt'),
    '-DCMAKE_RC_COMPILER=' + join(root, 'scripts/nsh-windows-rc.cjs'),
    '-DCMAKE_C_COMPILER_LAUNCHER=' + join(root, 'scripts/nsh-windows-clang.cjs'),
    '-DCMAKE_CXX_COMPILER_LAUNCHER=' + join(root, 'scripts/nsh-windows-clang.cjs'),
    '-DCMAKE_C_FLAGS=' + flags,
    '-DCMAKE_CXX_FLAGS=' + flags,
    '-DCMAKE_EXE_LINKER_FLAGS=' + linkFlags,
    '-DCMAKE_SHARED_LINKER_FLAGS=' + linkFlags,
    '-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded',
    '-DCMAKE_POLICY_DEFAULT_CMP0091=NEW',
  ];
}

function platformOptions(target, env) {
  if (target.os === 'windows') return windowsOptions(env);
  if (target.os === 'android') {
    const ndk = env.NDK_HOME ?? env.ANDROID_NDK_ROOT;
    if (!ndk) throw new Error('NDK_HOME is required for Android source rebuilds');
    return [
      '-DCMAKE_TOOLCHAIN_FILE=' + join(ndk, 'build/cmake/android.toolchain.cmake'),
      '-DANDROID_ABI=arm64-v8a',
      '-DANDROID_PLATFORM=android-' + target.minimumOs,
      '-DANDROID_STL=c++_static',
    ];
  }
  if (process.platform !== 'darwin') throw new Error('Apple source rebuild requires Xcode');
  const sdk =
    target.os === 'macos' ? 'macosx' : target.os === 'ios' ? 'iphoneos' : 'iphonesimulator';
  return [
    '-DCMAKE_OSX_SYSROOT=' +
      run('xcrun', ['--sdk', sdk, '--show-sdk-path'], { capture: true }).trim(),
    '-DCMAKE_OSX_ARCHITECTURES=' + target.arch,
    '-DCMAKE_OSX_DEPLOYMENT_TARGET=' + target.minimumOs,
    '-DCMAKE_INSTALL_NAME_DIR=@rpath',
    ...(target.os === 'macos'
      ? []
      : ['-DCMAKE_SYSTEM_NAME=iOS', '-DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY']),
  ];
}

export function buildHevc(triple) {
  const target = codecTarget(triple);
  const parent = join(root, 'target/native-codec-build');
  mkdirSync(parent, { recursive: true });
  const archive = join(parent, 'libde265-' + hevcSource.commit + '.tar.gz');
  if (!existsSync(archive)) {
    const download = mkdtempSync(join(parent, 'source-download-'));
    try {
      const path = join(download, 'source.tar.gz');
      run('curl', [
        '--fail',
        '--location',
        '--proto',
        '=https',
        '--proto-redir',
        '=https',
        '--max-time',
        '180',
        '--output',
        path,
        hevcSource.url,
      ]);
      if (createHash('sha256').update(readFileSync(path)).digest('hex') !== hevcSource.sha256)
        throw new Error('libde265 source download SHA-256 mismatch');
      renameSync(path, archive);
    } finally {
      rmSync(download, { recursive: true, force: true });
    }
  }
  if (createHash('sha256').update(readFileSync(archive)).digest('hex') !== hevcSource.sha256)
    throw new Error('libde265 source archive SHA-256 mismatch');
  const work = mkdtempSync(join(parent, triple + '-'));
  run('tar', ['-xzf', archive, '-C', work]);
  const source = join(work, 'libde265-' + hevcSource.commit);
  const build = join(work, 'build');
  const install = join(work, 'install');
  const env = { ...process.env };
  const options = [
    '-DCMAKE_BUILD_TYPE=Release',
    '-DCMAKE_EXPORT_COMPILE_COMMANDS=ON',
    '-DBUILD_SHARED_LIBS=' + (target.shared ? 'ON' : 'OFF'),
    '-DENABLE_DECODER=OFF',
    '-DENABLE_ENCODER=OFF',
    '-DENABLE_SDL=OFF',
    '-DENABLE_SHERLOCK265=OFF',
    '-DENABLE_INTERNAL_DEVELOPMENT_TOOLS=OFF',
    '-DWITH_FUZZERS=OFF',
    '-DCMAKE_INSTALL_PREFIX=' + install,
    ...(target.os === 'windows' ? ['-DHONGGUO_LIBDE265_SOURCE=' + source] : []),
    ...platformOptions(target, env),
  ];
  const entry = target.os === 'windows' ? join(root, 'scripts/native-codec-cmake') : source;
  run('cmake', ['-S', entry, '-B', build, '-G', 'Ninja', ...options], { env });
  run('cmake', ['--build', build, '--parallel', '4'], { env });
  run('cmake', ['--install', build], { env });
  writeFileSync(
    join(install, 'build-evidence.json'),
    JSON.stringify(
      {
        source: hevcSource,
        target,
        options,
        compiler: compilerIdentity(build),
        sourceArchive: archive,
        sourceDirectory: source,
        buildDirectory: build,
      },
      null,
      2,
    ) + '\n',
  );
  const pointer = join(parent, triple + '.json');
  writeFileSync(pointer, JSON.stringify({ install, work, target }, null, 2) + '\n');
  return install;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const targets = process.argv[2] === 'all' ? Object.keys(codecTargets) : [process.argv[2]];
  for (const triple of targets) console.log('Built codec: ' + triple + ' -> ' + buildHevc(triple));
}

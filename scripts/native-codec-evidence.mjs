import { codecTarget } from './native-codec-build.mjs';

export function sdkTarget(triple) {
  const target = codecTarget(triple);
  return {
    triple,
    arch: target.arch === 'arm64' ? 'aarch64' : target.arch,
    os: target.os === 'ios-simulator' ? 'ios' : target.os,
    minimumOs: target.minimumOs,
    crt: target.crt,
    cxxRuntime:
      target.os === 'windows'
        ? 'msvc-static'
        : target.os === 'android'
          ? 'libc++_static'
          : 'libc++',
    cpuBaseline: target.arch === 'x86_64' ? 'x86-64' : 'armv8-a',
    cppAbi: target.os === 'windows' ? 'msvc' : 'itanium',
  };
}

export function vvdecTarget(triple) {
  const target = sdkTarget(triple);
  if (target.os === 'macos') target.minimumOs = target.arch === 'x86_64' ? '10.15' : '11.0';
  return target;
}

function supportedVersion(value, maximum) {
  if (!/^\d+\.\d+(?:\.\d+)?$/.test(value)) return false;
  const actual = value.split('.').map(Number);
  const limit = String(maximum).split('.').map(Number);
  for (let index = 0; index < 3; index++) {
    if ((actual[index] ?? 0) !== (limit[index] ?? 0))
      return (actual[index] ?? 0) < (limit[index] ?? 0);
  }
  return true;
}

export function validateBuildEvidence(target, evidence) {
  const fail = (reason) => {
    throw new Error('VVdeC ' + target.triple + ': ' + reason);
  };
  if (evidence.configuration !== 'Release' || !evidence.sourceMatches)
    fail('requires a Release build of the current vendored source');
  const arch = target.arch === 'aarch64' && target.os !== 'android' ? 'arm64' : target.arch;
  if (evidence.architecture !== arch) fail('archive architecture does not match target');
  if (target.os === 'windows') {
    if (
      !evidence.machines?.length ||
      evidence.machines.some((machine) => machine !== 'IMAGE_FILE_MACHINE_AMD64')
    )
      fail('archive contains non-AMD64 objects');
    if (
      !/RuntimeLibrary=MT_StaticRelease/.test(evidence.directives) ||
      !/DEFAULTLIB:libcmt/i.test(evidence.directives) ||
      /DEFAULTLIB:(?:msvcrt|msvcprt)|RuntimeLibrary=MD_/i.test(evidence.directives)
    )
      fail('archive must use the static release MSVC CRT');
    if (/(?:\/arch:AVX|\s-mavx|\s-march=(?!x86-64\b))/.test(evidence.genericFlags ?? ''))
      fail('generic translation units exceed the CPU baseline');
    return;
  }
  if (target.os === 'android') {
    if (!evidence.machines?.length || evidence.machines.some((machine) => machine !== 'AArch64'))
      fail('archive contains non-AArch64 objects');
    if (evidence.compilerTarget !== 'aarch64-none-linux-android' + target.minimumOs)
      fail('compiler API does not match the SDK minimum');
    return;
  }
  const platform = target.os === 'macos' ? '1' : target.triple.endsWith('-sim') ? '7' : '2';
  if (!evidence.platforms?.length || evidence.platforms.some((value) => value !== platform))
    fail('Mach-O platform does not match target');
  if (
    !evidence.minimumVersions?.length ||
    evidence.minimumVersions.some((value) => !supportedVersion(value, target.minimumOs))
  )
    fail('Mach-O objects require a newer OS than the SDK declares');
}

export function validateHevcInspection(target, inspection, commands) {
  if (target.os === 'windows') {
    if (!inspection.includes('IMAGE_FILE_MACHINE_AMD64'))
      throw new Error('HEVC library is not AMD64');
    const generic = commands.find((entry) => /[/\\]de265\.cc$/.test(entry.file))?.command;
    if (
      !generic ||
      !/(?:^|\s)[/-]MT(?:\s|$)/.test(generic) ||
      /(?:\/arch:AVX|\s-mavx)/.test(generic)
    )
      throw new Error('HEVC generic objects must use static release CRT and the CPU baseline');
    return;
  }
  const expected = target.arch === 'aarch64' ? 'aarch64' : 'x86-64';
  const formats = [...inspection.matchAll(/^Format: (.+)$/gm)].map((match) => match[1]);
  if (
    !formats.length ||
    formats.some((format) =>
      target.os === 'android'
        ? format !== 'elf64-littleaarch64'
        : !format.includes(target.arch === 'aarch64' ? 'arm64' : expected),
    )
  )
    throw new Error('HEVC library has an unexpected target architecture');
  if (target.os === 'android') {
    if (
      !commands.length ||
      commands.some(
        (entry) =>
          !entry.command.includes('--target=aarch64-none-linux-android' + target.minimumOs),
      )
    )
      throw new Error('HEVC Android compiler API mismatch');
    return;
  }
  const platform =
    target.os === 'macos' ? 'macos' : target.triple.endsWith('-sim') ? 'iossimulator' : 'ios';
  const platforms = [...inspection.matchAll(/^\s+Platform: (.+)$/gm)].map((match) =>
    match[1].trim(),
  );
  const versions = [...inspection.matchAll(/^\s+Version: (.+)$/gm)].map((match) => match[1].trim());
  if (
    !platforms.length ||
    platforms.some((value) => value !== platform) ||
    !versions.length ||
    versions.some((value) => !supportedVersion(value, target.minimumOs))
  )
    throw new Error('HEVC Mach-O platform or minimum OS mismatch');
}

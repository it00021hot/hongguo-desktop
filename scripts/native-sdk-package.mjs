import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
} from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { codecTargets, hevcSource } from './native-codec-build.mjs';
import { sdkTarget, vvdecTarget, validateBuildEvidence } from './native-codec-evidence.mjs';
import { archiveSdk, digest, inventorySdk, jsonDigest, writeJson } from './native-codec-io.mjs';
import { stageHevc, stageVvdec, vvdecSource } from './native-codec-export.mjs';

export { archiveSdk, inventorySdk, sdkTarget, vvdecTarget, validateBuildEvidence };
export const sdkVersion = 'native-codecs-v2-20261114';
const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export function packageSdk(triple, outputDirectory = join(repository, 'vendor/native-codecs')) {
  const target = sdkTarget(triple);
  mkdirSync(outputDirectory, { recursive: true });
  const work = mkdtempSync(join(outputDirectory, '.codec-sdk-stage-'));
  try {
    const destination = join(work, triple);
    mkdirSync(destination);
    const hevcDirectory = 'libde265-' + hevcSource.version;
    const vvdecDirectory = 'vvdec-' + vvdecSource.version;
    const hevc = stageHevc(repository, join(destination, hevcDirectory), target);
    const vvdec = stageVvdec(repository, join(destination, vvdecDirectory), target);
    const recipe = {
      target,
      hevc: { source: hevcSource, build: hevc.build },
      vvdec: { source: vvdec.source, build: vvdec.build, target: vvdec.target },
    };
    const sdk = {
      schemaVersion: 1,
      sdkVersion,
      target,
      recipe,
      recipeSha256: jsonDigest(recipe),
      components: {
        hevc: {
          version: hevcSource.version,
          sourceSha256: hevcSource.sha256,
          manifestPath: hevcDirectory + '/manifest.json',
        },
        vvdec: {
          version: vvdecSource.version,
          sourceSha256: vvdecSource.sha256,
          manifestPath: vvdecDirectory + '/manifest.json',
        },
      },
      artifacts: inventorySdk(destination),
    };
    writeJson(join(destination, 'sdk.json'), sdk);
    const assetName = 'hongguo-codecs-v2-' + triple + '.tar.gz';
    const archive = join(outputDirectory, assetName);
    archiveSdk(destination, archive);
    const entry = {
      assetName,
      archivePath: 'vendor/native-codecs/' + assetName,
      sha256: digest(archive),
      byteLength: statSync(archive).size,
      manifestSha256: digest(join(destination, 'sdk.json')),
      recipeSha256: sdk.recipeSha256,
    };
    copyFileSync(join(destination, 'sdk.json'), join(outputDirectory, triple + '.sdk.json'));
    console.log(
      triple +
        ': exported ' +
        sdk.artifacts.length +
        ' codec artifacts, ' +
        entry.byteLength +
        ' archive bytes',
    );
    return entry;
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

function main() {
  const selection = process.argv[2] ?? 'all';
  const selected = selection === 'all' ? Object.keys(codecTargets) : [selection];
  const lockPath = join(repository, 'scripts/native-sdk.sources.json');
  const previous =
    selection !== 'all' && existsSync(lockPath)
      ? JSON.parse(readFileSync(lockPath, 'utf8'))
      : undefined;
  if (previous && previous.sdkVersion !== sdkVersion)
    throw new Error('Re-export all codec SDK targets when changing SDK versions');
  const targets = previous?.targets ?? {};
  for (const triple of selected) targets[triple] = packageSdk(triple);
  writeJson(lockPath, { schemaVersion: 1, sdkVersion, distribution: 'vendored', targets });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main();

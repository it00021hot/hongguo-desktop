import { readFileSync, writeFileSync, appendFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { parseArgs } from 'node:util';
import { createHash } from 'node:crypto';
import { join } from 'node:path';
import { DESKTOP_TARGETS } from './release-targets.mjs';

const { values } = parseArgs({
  options: { tag: { type: 'string' }, notes: { type: 'string' }, artifacts: { type: 'string' } },
});
const app = JSON.parse(readFileSync('crates/hongguo-nsh/nsh.json', 'utf8'));
const semver = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/;
if (!semver.test(app.version) || !Number.isSafeInteger(app.build) || app.build < 1)
  throw new Error('NSH version/build is invalid');

function command(program, args) {
  const result = spawnSync(program, args, { encoding: 'utf8' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${program} failed: ${result.stderr}`);
  return result.stdout.trim();
}

const metadata = JSON.parse(
  command('cargo', ['metadata', '--locked', '--no-deps', '--format-version', '1']),
);
const members = new Set(metadata.workspace_members);
const lock = readFileSync('Cargo.lock', 'utf8').replaceAll('\r\n', '\n');
for (const pkg of metadata.packages.filter((pkg) => members.has(pkg.id))) {
  if (pkg.version !== app.version)
    throw new Error(`Version mismatch: ${pkg.name}=${pkg.version}, NSH=${app.version}`);
  if (!lock.includes(`[[package]]\nname = "${pkg.name}"\nversion = "${pkg.version}"\n`))
    throw new Error(`Cargo.lock is not synchronized for ${pkg.name}`);
}

const changelog = readFileSync('CHANGELOG.md', 'utf8');
const sections = changelog.split(/^## /m).slice(1);
const section = sections.find((section) => section.startsWith(`[${app.version}] - `));
if (!section || !section.includes('\n- ')) throw new Error(`Missing changelog for ${app.version}`);
const sha = command('git', ['rev-parse', 'HEAD']);
if (values.tag) {
  if (values.tag !== `v${app.version}`) throw new Error(`Expected release tag v${app.version}`);
  const tagged = command('git', ['rev-parse', '--verify', `refs/tags/${values.tag}^{commit}`]);
  if (tagged !== sha) throw new Error('Release tag does not point to the checked-out commit');
}
if (values.notes) {
  writeFileSync(
    values.notes,
    `## ${section.trim()}\n\nWindows: unsigned Authenticode. macOS: ad hoc signed, not notarized.\n`,
  );
}
if (values.artifacts) {
  for (const [triple, target] of Object.entries(DESKTOP_TARGETS)) {
    const stem = `${app.name}-${app.version}-${target.platform}`;
    const manifestName = `${stem}.json`;
    const manifest = JSON.parse(readFileSync(join(values.artifacts, manifestName), 'utf8'));
    if (
      manifest.commit !== sha ||
      manifest.dirty !== false ||
      manifest.version !== app.version ||
      manifest.build !== app.build ||
      manifest.id !== app.id ||
      manifest.triple !== triple ||
      manifest.signing !== (target.os === 'win32' ? 'unsigned' : 'ad-hoc-not-notarized')
    )
      throw new Error(`Artifact provenance mismatch: ${target.platform}`);
    const expected =
      target.os === 'win32'
        ? [`${stem}-setup.exe`, `${stem}-portable.zip`]
        : [`${stem}.tar.gz`, `${stem}.dmg`];
    if (JSON.stringify(manifest.artifacts) !== JSON.stringify(expected))
      throw new Error(`Artifact list mismatch: ${target.platform}`);
    const checksumPath = join(values.artifacts, `SHA256SUMS-${target.platform}.txt`);
    const expectedChecksums =
      [...expected, manifestName]
        .map((name) => {
          const bytes = readFileSync(join(values.artifacts, name));
          if (!bytes.length) throw new Error(`Empty artifact: ${name}`);
          return `${createHash('sha256').update(bytes).digest('hex')}  ${name}`;
        })
        .join('\n') + '\n';
    if (readFileSync(checksumPath, 'utf8') !== expectedChecksums)
      throw new Error(`Artifact checksum mismatch: ${target.platform}`);
  }
}
if (process.env.GITHUB_OUTPUT) {
  appendFileSync(
    process.env.GITHUB_OUTPUT,
    `version=${app.version}\nsha=${sha}\ntag=v${app.version}\n`,
  );
}
console.log(`Release metadata verified: v${app.version}, build ${app.build}, ${sha}`);

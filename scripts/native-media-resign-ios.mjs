import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

function run(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command}: ${result.stderr}`);
  return result.stdout;
}

export function profileEntitlements(
  { expiresAt, prefix, granted },
  now = Date.now(),
  bundleId = 'com.hongguo.nsh',
) {
  const expiration = Date.parse(expiresAt);
  if (!Number.isFinite(expiration))
    throw new Error('The provisioning profile has no valid expiration date');
  if (expiration <= now) throw new Error('The provisioning profile has expired');
  if (typeof prefix !== 'string' || !prefix || !granted)
    throw new Error('The provisioning profile has no application identifier');
  const identifier = `${prefix}.${bundleId}`;
  const pattern = granted['application-identifier'];
  if (
    typeof pattern !== 'string' ||
    !(
      pattern === identifier ||
      (pattern.endsWith('*') && identifier.startsWith(pattern.slice(0, -1)))
    )
  )
    throw new Error(`The provisioning profile does not authorize ${bundleId}`);
  const team = granted['com.apple.developer.team-identifier'];
  if (typeof team !== 'string' || !team)
    throw new Error('The provisioning profile has no signing team');
  return {
    'application-identifier': identifier,
    'com.apple.developer.team-identifier': team,
    'get-task-allow': granted['get-task-allow'] === true,
  };
}

export function resignIos(app, binary, identity, profile) {
  const info = JSON.parse(run('plutil', ['-convert', 'json', '-o', '-', join(app, 'Info.plist')]));
  const executable = info.CFBundleExecutable;
  if (typeof executable !== 'string' || executable.includes('/') || executable.includes('\\'))
    throw new Error('Invalid app executable');
  const temporary = mkdtempSync(join(tmpdir(), 'hongguo-resign-'));
  try {
    const entitlements = join(temporary, 'entitlements.plist');
    const args = ['--force', '--sign', identity, '--timestamp=none'];
    if (identity !== '-') {
      if (!profile) throw new Error('Device signing requires --profile');
      const decoded = join(temporary, 'profile.plist');
      writeFileSync(decoded, run('security', ['cms', '-D', '-i', profile]));
      const data = JSON.parse(run('plutil', ['-convert', 'json', '-o', '-', decoded]));
      writeFileSync(
        entitlements,
        JSON.stringify(
          profileEntitlements(
            {
              expiresAt: data.ExpirationDate,
              prefix: data.ApplicationIdentifierPrefix?.[0],
              granted: data.Entitlements,
            },
            Date.now(),
            info.CFBundleIdentifier,
          ),
        ),
      );
      run('plutil', ['-convert', 'xml1', entitlements]);
      copyFileSync(profile, join(app, 'embedded.mobileprovision'));
      args.push('--entitlements', entitlements, '--generate-entitlement-der');
    } else if (!info.CFBundleSupportedPlatforms?.includes('iPhoneSimulator')) {
      throw new Error('Ad-hoc signing is only supported for the simulator app');
    }
    copyFileSync(binary, join(app, executable));
    run('codesign', [...args, app]);
    run('codesign', ['--verify', '--strict', app]);
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const options = {};
    const args = process.argv.slice(2);
    while (args.length) {
      const key = args.shift(),
        value = args.shift();
      if (!['--app', '--binary', '--identity', '--profile'].includes(key) || !value)
        throw new Error('Expected --app, --binary, --identity and device --profile');
      options[key.slice(2)] = value;
    }
    if (!options.app || !options.binary || !options.identity)
      throw new Error('Expected --app, --binary and --identity');
    resignIos(resolve(options.app), resolve(options.binary), options.identity, options.profile);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';

const COMPILE_SDK = 37;
const digest = (path) => createHash('sha256').update(readFileSync(path)).digest('hex');

function javaSources(directory) {
  return readdirSync(directory, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name))
    .flatMap((entry) => {
      const path = join(directory, entry.name);
      return entry.isDirectory() ? javaSources(path) : entry.name.endsWith('.java') ? [path] : [];
    });
}

export function buildAndroidRuntime({ aar, sdk, output, sourceDirectory }, run) {
  const platforms = join(sdk, 'platforms');
  const candidates = existsSync(platforms)
    ? readdirSync(platforms).flatMap((name) => {
        const properties = join(platforms, name, 'source.properties');
        if (!existsSync(properties)) return [];
        const text = readFileSync(properties, 'utf8');
        if (
          !new RegExp(`^AndroidVersion.ApiLevel=${COMPILE_SDK}(?:\\.0)?\\r?$`, 'm').test(text) ||
          /^AndroidVersion.CodeName=[^\r\n]+/m.test(text)
        )
          return [];
        return [
          {
            path: join(platforms, name, 'android.jar'),
            revision: /^Pkg.Revision=([^\r\n]+)$/m.exec(text)?.[1] ?? '',
          },
        ];
      })
    : [];
  candidates.sort((a, b) => b.revision.localeCompare(a.revision, undefined, { numeric: true }));
  const platform = candidates[0]?.path;
  if (!platform || !existsSync(platform))
    throw new Error(`Android SDK ${COMPILE_SDK} is required: ${platforms}`);
  const sources = javaSources(sourceDirectory);
  if (!sources.length) throw new Error(`NSH Android Java sources are missing: ${sourceDirectory}`);
  const work = join(output, 'runtime-java');
  rmSync(work, { recursive: true, force: true });
  const classes = join(work, 'compiled');
  mkdirSync(classes, { recursive: true });
  const entries = ['AndroidManifest.xml', 'R.txt', 'proguard.txt'];
  run('jar', ['xf', aar, ...entries], {}, work);
  const args = [
    '--release',
    '17',
    '-encoding',
    'UTF-8',
    '-classpath',
    platform,
    '-d',
    classes,
    ...sources,
  ];
  const response = join(work, 'javac.args');
  writeFileSync(
    response,
    args.map((value) => JSON.stringify(value.replaceAll('\\', '/'))).join('\n'),
  );
  run('javac', [`@${response}`]);
  const jar = join(work, 'classes.jar');
  run('jar', ['cf', jar, '-C', classes, '.']);
  const runtime = join(output, 'nsh-runtime.aar');
  run('jar', ['cf', runtime, 'classes.jar', ...entries], {}, work);
  writeFileSync(
    join(output, 'runtime-source.json'),
    JSON.stringify(
      {
        schemaVersion: 1,
        compileSdk: COMPILE_SDK,
        embeddingSdkSha256: digest(aar),
        androidJarSha256: digest(platform),
        runtimeSha256: digest(runtime),
        classesSha256: digest(jar),
        sources: sources.map((path) => ({
          path: relative(sourceDirectory, path).replaceAll('\\', '/'),
          sha256: digest(path),
        })),
      },
      null,
      2,
    ),
  );
  return runtime;
}

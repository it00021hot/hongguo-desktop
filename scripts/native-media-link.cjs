#!/usr/bin/env node
(async () => {
  const { captureLink } = await import('./native-media-relink.mjs');
  captureLink(process.argv.slice(2));
})().catch((error) => {
  process.stderr.write(`${error.message}\n`);
  process.exitCode = 1;
});

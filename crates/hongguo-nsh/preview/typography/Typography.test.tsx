import { expect, open, test as registerTest } from 'nsh/test';

const test: (name: string, body: () => Promise<void>) => void = registerTest;

test('native weight and family diagnostics', async () => {
  const app = await open('.', { width: 1000, height: 450, colorScheme: 'dark', locale: 'zh-CN' });
  expect(app.shows('text:400')).toBe(true);
  await app.screenshot('target/nsh-preview/typography-native.png');
});

import { describe, expect, it } from 'vitest';
import { queryUpdate } from './update-gate';

describe('queryUpdate', () => {
  it('启动和手动都在调用插件之前返回', async () => {
    const holder = globalThis as { __HONGGUO_OFFLINE_QA?: string };
    holder.__HONGGUO_OFFLINE_QA = '1';
    let calls = 0;
    const check = () => {
      calls += 1;
      return Promise.resolve({ version: '9' });
    };
    await queryUpdate(check);
    await queryUpdate(check);
    expect(calls).toBe(0);
    holder.__HONGGUO_OFFLINE_QA = '0';
    await queryUpdate(check);
    expect(calls).toBe(1);
    delete holder.__HONGGUO_OFFLINE_QA;
  });
});

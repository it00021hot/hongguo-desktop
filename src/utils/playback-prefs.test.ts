import { describe, expect, it, beforeEach } from 'vitest';
import {
  MAX_RATE,
  MIN_RATE,
  readMuted,
  readPlaybackRate,
  readVolume,
  writeMuted,
  writePlaybackRate,
  writeVolume,
} from './playback-prefs';

/**
 * 测试环境是 node，没有 window.localStorage。
 * 这里塞一个最小实现——本模块只在函数里读它，测试再晚装也能生效。
 * 不为此引入 jsdom：一个存储桩不值得多一个依赖。
 */
function installLocalStorage() {
  const map = new Map<string, string>();
  const storage = {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
    clear: () => map.clear(),
  };
  (globalThis as { window?: unknown }).window = { localStorage: storage };
  return storage;
}

let storage: ReturnType<typeof installLocalStorage>;

/** 切集会重建 <video>，新建元素的倍速恒为 1，所以这些值必须跨会话留存。 */
describe('playback-prefs', () => {
  beforeEach(() => {
    storage = installLocalStorage();
  });

  it('首次读取回落到默认值', () => {
    expect(readPlaybackRate()).toBe(1);
    expect(readVolume()).toBe(1);
    expect(readMuted()).toBe(false);
  });

  it('倍速能跨会话读回', () => {
    writePlaybackRate(2);
    expect(readPlaybackRate()).toBe(2);
  });

  it('音量能跨会话读回', () => {
    writeVolume(0.4);
    expect(readVolume()).toBeCloseTo(0.4);
  });

  it('静音是独立状态，不会被音量归零顶掉', () => {
    // 用户静音后 volume 仍是原值，恢复时不能把静音悄悄取消
    writeVolume(0.8);
    writeMuted(true);
    expect(readVolume()).toBeCloseTo(0.8);
    expect(readMuted()).toBe(true);
  });

  it('越界的倍速被夹回合法区间而不是照单全收', () => {
    writePlaybackRate(999);
    expect(readPlaybackRate()).toBe(1);
    writePlaybackRate(0.01);
    expect(readPlaybackRate()).toBe(1);
  });

  it('损坏的存储值回落到默认值', () => {
    storage.setItem('hongguo.playbackRate', '不是数字');
    expect(readPlaybackRate()).toBe(1);
  });

  it('合法区间边界是可接受的', () => {
    writePlaybackRate(MIN_RATE);
    expect(readPlaybackRate()).toBeCloseTo(MIN_RATE);
    writePlaybackRate(MAX_RATE);
    expect(readPlaybackRate()).toBeCloseTo(MAX_RATE);
  });
});

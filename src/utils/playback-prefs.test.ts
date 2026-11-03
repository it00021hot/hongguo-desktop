import { describe, expect, it, beforeEach } from 'vitest';
import {
  MAX_RATE,
  MIN_RATE,
  readEnhancement,
  readMuted,
  readPlaybackRate,
  readVolume,
  writeEnhancement,
  writeMuted,
  writePlaybackRate,
  writeVolume,
} from './playback-prefs';

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

  it('画质增强默认全关、强度 50', () => {
    expect(readEnhancement()).toEqual({ anime4kOn: false, sharpenOn: false, strength: 50 });
  });

  it('画质增强设置能跨会话读回（两开关独立）', () => {
    writeEnhancement({ anime4kOn: true, sharpenOn: true, strength: 80 });
    expect(readEnhancement()).toEqual({ anime4kOn: true, sharpenOn: true, strength: 80 });
    writeEnhancement({ anime4kOn: false, sharpenOn: true, strength: 30 });
    expect(readEnhancement()).toEqual({ anime4kOn: false, sharpenOn: true, strength: 30 });
  });

  it('非布尔开关与越界强度被归一化，不照单全收', () => {
    storage.setItem(
      'hongguo.enhance',
      JSON.stringify({ anime4kOn: 'yes', sharpenOn: 1, strength: 999 }),
    );
    expect(readEnhancement()).toEqual({ anime4kOn: false, sharpenOn: false, strength: 100 });
    storage.setItem(
      'hongguo.enhance',
      JSON.stringify({ anime4kOn: true, sharpenOn: true, strength: -3 }),
    );
    expect(readEnhancement()).toEqual({ anime4kOn: true, sharpenOn: true, strength: 0 });
  });

  it('损坏的画质增强存储值回落到默认值', () => {
    storage.setItem('hongguo.enhance', '不是JSON');
    expect(readEnhancement()).toEqual({ anime4kOn: false, sharpenOn: false, strength: 50 });
    storage.setItem('hongguo.enhance', JSON.stringify({ anime4kOn: true }));
    expect(readEnhancement()).toEqual({ anime4kOn: false, sharpenOn: false, strength: 50 });
  });
});

import { describe, expect, it } from 'vitest';
import { formatBytes, formatCountPrecise, formatDuration } from './format';

describe('formatBytes', () => {
  it('零与负数', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(-1)).toBe('0 B');
  });

  it('按 1024 进位', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1024)).toBe('1.0 KB');
    expect(formatBytes(1024 * 1024 * 5)).toBe('5.0 MB');
    expect(formatBytes(1024 ** 3)).toBe('1.0 GB');
  });
});

describe('formatDuration', () => {
  it('分钟以内', () => {
    expect(formatDuration(0)).toBe('00:00');
    expect(formatDuration(65)).toBe('01:05');
  });

  it('超过一小时带小时位', () => {
    expect(formatDuration(3661)).toBe('1:01:01');
  });

  it('非法输入', () => {
    expect(formatDuration(-1)).toBe('00:00');
    expect(formatDuration(Number.NaN)).toBe('00:00');
  });
});

describe('formatCountPrecise', () => {
  // 口径对齐 hgplayer 详情头部：27.1万人追剧 / 40.3万次播放 / 热度值3786万
  it('万位带一位小数，整数省略小数点', () => {
    expect(formatCountPrecise(270_523)).toBe('27.1万');
    expect(formatCountPrecise(402_591)).toBe('40.3万');
    expect(formatCountPrecise(37_860_097)).toBe('3786万');
  });

  it('亿位与万位以下', () => {
    expect(formatCountPrecise(150_000_000)).toBe('1.5亿');
    expect(formatCountPrecise(1247)).toBe('1247');
  });
});

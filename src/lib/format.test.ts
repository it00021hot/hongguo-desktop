import { describe, expect, it } from 'vitest';
import { formatBytes, formatDuration } from './format';

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

import { describe, expect, it } from 'vitest';
import { formatBytes, formatDuration, renderFileName, sanitizeFileName } from './format';

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

describe('sanitizeFileName', () => {
  it('替换路径分隔符与保留字符', () => {
    expect(sanitizeFileName('a/b\\c:d')).toBe('a_b_c_d');
    expect(sanitizeFileName('剧*名?')).toBe('剧_名_');
  });

  it('去掉结尾的点与空格', () => {
    expect(sanitizeFileName('剧名. ')).toBe('剧名');
  });

  it('空串回落到未命名', () => {
    expect(sanitizeFileName('')).toBe('未命名');
  });
});

describe('renderFileName', () => {
  it('剧名 + 集数（补零）', () => {
    expect(renderFileName('titleIndex', '剧名', 7, '标题')).toBe('剧名 007');
  });

  it('剧名 + 集数 + 标题', () => {
    expect(renderFileName('titleIndexEpisode', '剧名', 7, '标题')).toBe('剧名 007 标题');
  });

  it('标题为空时省略', () => {
    expect(renderFileName('titleIndexEpisode', '剧名', 7, '  ')).toBe('剧名 007');
  });

  it('仅剧名', () => {
    expect(renderFileName('onlyTitle', '剧名', 7, '标题')).toBe('剧名');
  });
});

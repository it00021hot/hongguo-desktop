import { describe, expect, it } from 'vitest';
import { firstN, formatRange, lastN, parseRange } from './range';

describe('parseRange', () => {
  it('解析单个数字', () => {
    expect(parseRange('5')).toEqual([5]);
  });

  it('解析闭区间', () => {
    expect(parseRange('1-5')).toEqual([1, 2, 3, 4, 5]);
  });

  it('解析混合表达式', () => {
    expect(parseRange('1-3, 7, 10-12')).toEqual([1, 2, 3, 7, 10, 11, 12]);
  });

  it('去重并排序', () => {
    expect(parseRange('3, 1, 3, 2')).toEqual([1, 2, 3]);
  });

  it('空串返回空数组', () => {
    expect(parseRange('')).toEqual([]);
    expect(parseRange('   ')).toEqual([]);
  });

  it('忽略非法片段', () => {
    expect(parseRange('abc, 5')).toEqual([5]);
    expect(parseRange('5-1')).toEqual([]);
    expect(parseRange('0')).toEqual([]);
  });

  it('区间上界被 max 夹住', () => {
    expect(parseRange('1-100', 10)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
  });

  it('超出 max 的单个数字被丢弃', () => {
    expect(parseRange('11', 10)).toEqual([]);
  });
});

describe('formatRange', () => {
  it('连续段压成区间', () => {
    expect(formatRange([1, 2, 3, 7, 10, 11])).toBe('1-3, 7, 10-11');
  });

  it('单个数字不带区间', () => {
    expect(formatRange([5])).toBe('5');
  });

  it('空数组返回空串', () => {
    expect(formatRange([])).toBe('');
  });

  it('与 parseRange 互为逆运算', () => {
    const input = '1-3, 7, 10-12';
    expect(formatRange(parseRange(input))).toBe(input);
  });
});

describe('firstN / lastN', () => {
  it('取前 N 集', () => {
    expect(firstN(20, 3)).toEqual([1, 2, 3]);
  });

  it('N 超过总数时取全部', () => {
    expect(firstN(3, 10)).toEqual([1, 2, 3]);
  });

  it('取后 N 集', () => {
    expect(lastN(20, 3)).toEqual([18, 19, 20]);
  });

  it('总数为 0 时返回空数组', () => {
    expect(firstN(0, 5)).toEqual([]);
    expect(lastN(0, 5)).toEqual([]);
  });
});

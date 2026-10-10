import { describe, expect, it } from 'vitest';
import { DANMAKU_EMOJI_LIST, parseEmojiSegments } from './danmaku-emoji';

describe('danmaku emoji', () => {
  it('表情表齐全：53 个，全部有资源 URL', () => {
    expect(DANMAKU_EMOJI_LIST).toHaveLength(53);
    for (const e of DANMAKU_EMOJI_LIST) {
      expect(e.name).toMatch(/^\[[^\][\s]{1,8}\]$/);
      expect(e.url).not.toBe('');
    }
  });

  it('上游真实样本：文本与 [爱慕] 混排', () => {
    const segs = parseEmojiSegments('我去，开头好美[爱慕][爱慕][爱慕]');
    expect(segs).toEqual([
      { kind: 'text', value: '我去，开头好美' },
      { kind: 'emoji', value: '[爱慕]', url: expect.any(String) },
      { kind: 'emoji', value: '[爱慕]', url: expect.any(String) },
      { kind: 'emoji', value: '[爱慕]', url: expect.any(String) },
    ]);
  });

  it('纯文本不加段', () => {
    expect(parseEmojiSegments('前方高能')).toEqual([{ kind: 'text', value: '前方高能' }]);
    expect(parseEmojiSegments('')).toEqual([]);
  });

  it('不认识的代码原样保留', () => {
    expect(parseEmojiSegments('笑[不存在的表情]了')).toEqual([
      { kind: 'text', value: '笑[不存在的表情]了' },
    ]);
  });

  it('代码长度 1-8 之外不匹配', () => {
    expect(parseEmojiSegments('[九个字以上的名字]')).toEqual([
      { kind: 'text', value: '[九个字以上的名字]' },
    ]);
    expect(parseEmojiSegments('[赞]')).toEqual([
      { kind: 'emoji', value: '[赞]', url: expect.any(String) },
    ]);
  });
});

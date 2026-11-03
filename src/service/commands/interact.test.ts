import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@/locales', () => ({ t: (k: string) => k, tf: (k: string) => k }));

import { interact } from './interact';

describe('interact.sendComment 入参键名', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({
      commentId: 'c1',
      userId: 'u1',
      userName: 'n',
      avatar: '',
      text: 't',
      createTime: 0,
      diggCount: 0,
      replyCount: 0,
      userDigg: false,
      score: '',
      scoreSuffixText: '',
    });
  });

  it('提及参数走 mentionUserId / mentionUserName / contextCommentIds（不是 userId）', async () => {
    await interact.sendComment('vid1', 'series1', '正文', {
      userId: '2029014865965927',
      userName: '小果',
      contextCommentIds: ['c1', 'c2'],
    });
    expect(invokeMock).toHaveBeenCalledWith('comment_send', {
      groupId: 'vid1',
      bookId: 'series1',
      text: '正文',
      mentionUserId: '2029014865965927',
      mentionUserName: '小果',
      contextCommentIds: ['c1', 'c2'],
    });
  });

  it('无提及时提及参数为 undefined（JSON 序列化后不会到达 Tauri）', async () => {
    await interact.sendComment('vid1', 'series1', '正文');
    const args = (invokeMock.mock.lastCall?.[1] ?? {}) as Record<string, unknown>;
    expect(args).toEqual({ groupId: 'vid1', bookId: 'series1', text: '正文' });
    expect(args.mentionUserId).toBeUndefined();
    expect(args.mentionUserName).toBeUndefined();
    expect(args.contextCommentIds).toBeUndefined();
  });
});

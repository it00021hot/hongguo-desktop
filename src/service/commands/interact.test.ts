import { beforeEach, describe, expect, it, vi } from 'vitest';

// call() 只校验响应不校验入参——comment_send 的提及参数键名写错会被
// Tauri 静默丢弃、@小果AI 降级成普通评论（2026-10-16 实锤事故）。
// 这里锁死「service 层发出的键名 = registry 声明的键名」。
const invokeMock = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
// invoke.ts 依赖 locales（node 环境无 localStorage），mock 断开
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

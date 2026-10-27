import { describe, expect, it } from 'vitest';
import {
  commentItemSchema,
  commentPageSchema,
  danmakuSchema,
  replyPageSchema,
  seriesReviewPageSchema,
} from './danmaku';
import { downloadProgressSchema, downloadTaskSchema } from './download';
import { loginMfaStateSchema } from './login';
import { rankPageSchema } from './rank';

/**
 * 响应契约 fixture：按 Rust serde 模型的既有形状（出处见
 * `docs/hongguo-api-endpoints.md` 与各 schema 注释）手建的「 Rust 会产出
 * 什么」。这层网防的是**前端 schema 被无意改严/改形**与 serde 漂移——
 * Rust 模型改动当天这里会红（配合原计划批次三的 Rust 侧 struct 化）。
 */

const commentItem = {
  commentId: '7350000000000000001',
  userName: '用户甲',
  avatar: 'https://p3-sign.example.com/img.jpg',
  text: '好看',
  createTime: 1791603887000,
  diggCount: 3,
  replyCount: 1,
  userDigg: false,
};

describe('弹幕/评论/剧评契约', () => {
  it('commentItem：完整字段过校验，缺省字段补默认', () => {
    const parsed = commentItemSchema.parse(commentItem);
    // userId/score/scoreSuffixText 是 default 字段
    expect(parsed.userId).toBe('');
    expect(parsed.score).toBe('');
  });

  it('commentPage：拒绝缺 total 的响应（serde 漂移当场暴露）', () => {
    expect(
      commentPageSchema.safeParse({
        items: [commentItem],
        hasMore: false,
        nextCursor: '',
      }).success,
    ).toBe(false);
    expect(
      commentPageSchema.safeParse({
        items: [commentItem],
        total: 42,
        hasMore: true,
        nextCursor: '6500000000000000009',
      }).success,
    ).toBe(true);
  });

  it('seriesReviewPage：剧评分摘要字段齐全', () => {
    expect(
      seriesReviewPageSchema.safeParse({
        items: [commentItem],
        total: 1074,
        hasMore: false,
        nextCursor: '',
        score: '8.0',
        scoreCnt: 1074,
        tags: ['修仙'],
        tagStats: [{ tagName: '修仙世界观宏大', count: 26 }],
      }).success,
    ).toBe(true);
  });

  it('replyPage / danmaku：最小合法形状', () => {
    const replyItem = {
      replyId: '7350000000000000009',
      userName: '用户乙',
      avatar: 'https://p3-sign.example.com/img2.jpg',
      text: '同意',
      createTime: 1791603900000,
      diggCount: 0,
      userDigg: false,
    };
    expect(
      replyPageSchema.safeParse({
        items: [replyItem],
        total: 1,
        hasMore: false,
        nextCursor: '',
      }).success,
    ).toBe(true);
    expect(
      danmakuSchema.safeParse({ commentId: '1', text: '前方高能', offsetMs: 12000, diggCount: 0 })
        .success,
    ).toBe(true);
  });
});

describe('登录 MFA 事件三态', () => {
  it('三态各过各的，state 串票失败', () => {
    expect(loginMfaStateSchema.safeParse({ state: 'success', name: '用户甲' }).success).toBe(true);
    expect(
      loginMfaStateSchema.safeParse({
        state: 'failed',
        messageKind: 'settings.loginMfaTimeout',
        message: '',
      }).success,
    ).toBe(true);
    expect(loginMfaStateSchema.safeParse({ state: 'waiting', serverTip: '回复短信' }).success).toBe(
      true,
    );
    // Rust 侧只发这三种；出现别的 state 是契约漂移
    expect(loginMfaStateSchema.safeParse({ state: 'pending' }).success).toBe(false);
  });
});

describe('下载域契约', () => {
  it('downloadTask：Rust DownloadTask serde 形状', () => {
    expect(
      downloadTaskSchema.safeParse({
        id: '1',
        seriesId: '7000000000000000001',
        seriesTitle: '某剧',
        vidIndex: 1,
        vid: '7100000000000000001',
        epTitle: '第1集',
        filePath: 'C:/x/1.mp4',
        tempPath: 'C:/x/1.mp4.part',
        status: 'running',
        downloaded: 1024,
        total: 2048,
        error: '',
        createdAt: 1791603887,
        updatedAt: 1791603888,
      }).success,
    ).toBe(true);
  });

  it('downloadProgress：scheduler 四字段负载', () => {
    expect(
      downloadProgressSchema.safeParse({ id: '1', downloaded: 1, total: 2, percent: 50 }).success,
    ).toBe(true);
    // Rust 端 percent 恒有（clamp 到 0.0），缺字段即漂移
    expect(downloadProgressSchema.safeParse({ id: '1', downloaded: 1, total: 2 }).success).toBe(
      false,
    );
  });
});

describe('榜单契约', () => {
  it('rankPage：tabs/分页游标缺省时补默认（服务端老版本不下发）', () => {
    const parsed = rankPageSchema.parse({ items: [], tabs: [] });
    expect(parsed.hasMore).toBe(false);
    expect(parsed.nextOffset).toBe(0);
    expect(parsed.sessionId).toBe('');
  });
});

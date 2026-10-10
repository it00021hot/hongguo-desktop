import { describe, expect, it } from 'vitest';
import { normalizeAvatarUrl } from './image-url';

describe('normalizeAvatarUrl', () => {
  it('http 签名头像改写 https（签名不失效，绕开 CSP/ATS）', () => {
    const u =
      'http://p3-reading-sign.fqnovelpic.com/tos-cn-i-1yzifmftcy/8623cf7debd14a7b8eedaa2de4e3d6c7~tplv-s85hriknmn-jpeg.jpeg?lk3s=9b9bd2b1&x-signature=bvSZFjER';
    expect(normalizeAvatarUrl(u)).toBe(`https://${u.slice(7)}`);
  });

  it('.heic 后缀改写 .jpeg 后仍是显式可渲染扩展名，直连不走代理', () => {
    const out = normalizeAvatarUrl(
      'https://p3-novel.byteimg.com/img/novel-static/51cdc818263941cb8985f97b486ddd11~tplv-obj.heic',
    );
    expect(out).toBe(
      'https://p3-novel.byteimg.com/img/novel-static/51cdc818263941cb8985f97b486ddd11~tplv-obj.jpeg',
    );
  });

  it('带 query 的 jpeg/png/webp 原样直连', () => {
    const u =
      'https://p26.douyinpic.com/aweme/1080x1080/aweme-avatar/tos-cn-avt-0015_2a8d.jpeg?from=3782654143';
    expect(normalizeAvatarUrl(u)).toBe(u);
  });

  it('无扩展名地址走 hongguo-cover 本地代理（格式未知，后端嗅探兜底）', () => {
    const out = normalizeAvatarUrl(
      'https://p6-novel.byteimg.com/large/novel-static/c777c29b820fc2081bb346ecb497f450',
    );
    expect(out).toContain('hongguo-cover');
    expect(out).toContain('/c/');
  });

  it('passport 的 .image 扩展名同样走代理', () => {
    const out = normalizeAvatarUrl(
      'https://p9-passport.byteacctimg.com/img/mosaic-legacy/3791/5035712059~120x256.image',
    );
    expect(out).toContain('hongguo-cover');
  });

  it('空串原样返回', () => {
    expect(normalizeAvatarUrl('')).toBe('');
  });
});

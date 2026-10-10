import { describe, expect, it } from 'vitest';
import { heicUrlToJpeg } from './image-url';

describe('heicUrlToJpeg', () => {
  it('把 .heic 后缀改写成 .jpeg（CDN 服务端转码）', () => {
    expect(
      heicUrlToJpeg(
        'https://p3-novel.byteimg.com/img/novel-static/51cdc818263941cb8985f97b486ddd11~tplv-obj.heic',
      ),
    ).toBe(
      'https://p3-novel.byteimg.com/img/novel-static/51cdc818263941cb8985f97b486ddd11~tplv-obj.jpeg',
    );
  });

  it('非 .heic 的 URL 原样返回', () => {
    const u = 'https://p9-passport.byteacctimg.com/img/user-avatar.jpg';
    expect(heicUrlToJpeg(u)).toBe(u);
  });

  it('路径中段含 .heic 的不算（只看后缀）', () => {
    const u = 'https://example.com/heic/foo.jpg';
    expect(heicUrlToJpeg(u)).toBe(u);
  });
});

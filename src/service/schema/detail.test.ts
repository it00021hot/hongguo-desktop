import { describe, expect, it } from 'vitest';

import { seriesMetaSchema } from './detail';

describe('详情元信息契约', () => {
  const baseMeta = {
    seriesId: '7678638019280112702',
    title: '末日宿舍求生',
    cover: 'https://p3-novel.byteimg.com/cover.jpeg',
    followedCnt: 120000,
    playCnt: 1508000,
    recordNumber: '',
    season: '',
    tags: ['末世'],
  };

  const creator = (avatar: string) => ({
    userId: '2007062623837002',
    name: '郡园',
    avatar,
    role: '主演',
  });

  it('creators[].avatar：签名 http 地址改写成 https（直挂可显）', () => {
    const signed =
      'http://p3-reading-sign.fqnovelpic.com/novel-pic/16173afabdc8b29d587e35caf0cc146f~tplv-s85hriknmn-jpeg-v1:144:0.jpeg?lk3s=9b9bd2b1&x-expires=1795450105&x-signature=abc%3D';
    const parsed = seriesMetaSchema.parse({
      ...baseMeta,
      creators: [creator(signed)],
    });
    expect(parsed.creators[0]?.avatar).toBe(`https://${signed.slice('http://'.length)}`);
  });

  it('creators[].avatar：无扩展名地址走本地代理（格式说不清，后端嗅探）', () => {
    const parsed = seriesMetaSchema.parse({
      ...baseMeta,
      creators: [creator('https://p6-novel.byteimg.com/large/novel-static/c777c29b')],
    });
    expect(parsed.creators[0]?.avatar).toContain('hongguo-cover');
  });

  it('creators[].avatar：已是 https jpeg 的地址原样保留（不绕代理）', () => {
    const direct = 'https://p26.douyinpic.com/aweme/1080x1080/aweme-avatar/a.jpeg';
    const parsed = seriesMetaSchema.parse({
      ...baseMeta,
      creators: [creator(direct)],
    });
    expect(parsed.creators[0]?.avatar).toBe(direct);
  });

  it('creators 缺席时仍给空数组（旧后端窗口期）', () => {
    const parsed = seriesMetaSchema.parse(baseMeta);
    expect(parsed.creators).toEqual([]);
  });
});

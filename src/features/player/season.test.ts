import { describe, expect, it } from 'vitest';

import type { RelatedItem } from '@/service/schema';
import { pickNextSeason, seasonFromTitle, seasonNoFromTag } from './season';

function work(seriesId: string, title: string, tag: string, episodeCnt = 12): RelatedItem {
  return {
    seriesId,
    title,
    cover: '',
    tag,
    score: 0,
    playCnt: 0,
    episodeCnt,
    videoDesc: '',
  };
}

const NIJIANGJIANG = '逆江湖之苍生为局';

describe('seasonNoFromTag', () => {
  it('只认全匹配的「第N季」阿拉伯形态', () => {
    expect(seasonNoFromTag('第1季')).toBe(1);
    expect(seasonNoFromTag('第12季')).toBe(12);
    expect(seasonNoFromTag('第三季')).toBeNull();
    expect(seasonNoFromTag('同IP')).toBeNull();
    expect(seasonNoFromTag('即将上线')).toBeNull();
    expect(seasonNoFromTag('')).toBeNull();
  });
});

describe('seasonFromTitle', () => {
  it('阿拉伯与中文数字都认', () => {
    expect(seasonFromTitle('逆江湖之苍生为局第3季')).toBe(3);
    expect(seasonFromTitle('逆江湖之苍生为局第三季')).toBe(3);
    expect(seasonFromTitle('某剧第十二季')).toBe(12);
    expect(seasonFromTitle('某剧第二十一季')).toBe(21);
    expect(seasonFromTitle('某剧第两季')).toBe(2);
  });

  it('无季标返回 null', () => {
    expect(seasonFromTitle(NIJIANGJIANG)).toBeNull();
  });
});

describe('pickNextSeason', () => {
  it('列表乱序也按季数接：S3 的下一季是 S4，不是服务端顺序的下一条', () => {
    const works = [
      work('s3', `${NIJIANGJIANG}第三季`, '第3季'),
      work('s5', `${NIJIANGJIANG}第五季`, '第5季'),
      work('s4', `${NIJIANGJIANG}第四季`, '第4季'),
    ];
    expect(pickNextSeason(works, 's3', `${NIJIANGJIANG}第三季`)?.seriesId).toBe('s4');
  });

  it('同 IP 作品（无季角标）不是候选', () => {
    const works = [
      work('s3', `${NIJIANGJIANG}第三季`, '第3季'),
      work('ip1', '同IP另一部剧', '同IP'),
      work('s4', `${NIJIANGJIANG}第四季`, '第4季'),
    ];
    expect(pickNextSeason(works, 's3', `${NIJIANGJIANG}第三季`)?.seriesId).toBe('s4');
  });

  it('未上线季（episodeCnt 0）不当候选：S4 预约中时接 S5', () => {
    const works = [
      work('s3', `${NIJIANGJIANG}第三季`, '第3季'),
      work('s4', `${NIJIANGJIANG}第四季`, '第4季', 0),
      work('s5', `${NIJIANGJIANG}第五季`, '第5季'),
    ];
    expect(pickNextSeason(works, 's3', `${NIJIANGJIANG}第三季`)?.seriesId).toBe('s5');
  });

  it('当前季不在 works 里（plan/v 只回其他季）且标题带季数：照季数接', () => {
    const works = [
      work('s1', `${NIJIANGJIANG}第一季`, '第1季'),
      work('s2', `${NIJIANGJIANG}第二季`, '第2季'),
      work('s4', `${NIJIANGJIANG}第四季`, '第4季'),
    ];
    expect(pickNextSeason(works, 'sX', `${NIJIANGJIANG}第三季`)?.seriesId).toBe('s4');
  });

  it('当前是无标第一季（标题无季数、不在 works）：接最小季', () => {
    const works = [
      work('s2', `${NIJIANGJIANG}第二季`, '第2季'),
      work('s3', `${NIJIANGJIANG}第三季`, '第3季'),
    ];
    expect(pickNextSeason(works, 's1', NIJIANGJIANG)?.seriesId).toBe('s2');
  });

  it('当前季在 works 里但无角标：标题解析兜底', () => {
    const works = [
      work('s3', `${NIJIANGJIANG}第三季`, ''),
      work('s4', `${NIJIANGJIANG}第四季`, '第4季'),
    ];
    expect(pickNextSeason(works, 's3', `${NIJIANGJIANG}第三季`)?.seriesId).toBe('s4');
  });

  it('已是最后一季：返回 null（调用方走猜你喜欢回落）', () => {
    const works = [
      work('s2', `${NIJIANGJIANG}第二季`, '第2季'),
      work('s3', `${NIJIANGJIANG}第三季`, '第3季'),
    ];
    expect(pickNextSeason(works, 's3', `${NIJIANGJIANG}第三季`)).toBeNull();
  });

  it('列表没有任何带标季：返回 null', () => {
    const works = [work('ip1', '同IP另一部剧', '同IP')];
    expect(pickNextSeason(works, 's1', NIJIANGJIANG)).toBeNull();
  });
});

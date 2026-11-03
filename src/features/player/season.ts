//! 同系列各季的季数解析与「下一季」挑选。
//!
//! 数据面是 plan/v 的相关作品列表（`RelatedItem.works`）：同系列各季带「第N季」
//! 角标，同 IP 作品不带。服务端顺序不可信，「列表下一条」会跳季/倒退，接力选季
//! 一律走季数：大于当前季的最小已上线季（episodeCnt 0 = 未上线，不当候选）。

import type { RelatedItem } from '@/service/schema';

/** 「第N季」角标（阿拉伯数字，plan/v 响应的形态）。 */
export function seasonNoFromTag(tag: string): number | null {
  const m = /^第(\d{1,2})季$/.exec(tag);
  return m ? Number(m[1]) : null;
}

/** 中文数字 → 阿拉伯（一~九十九够用：季数不会上百）。 */
function chineseNumeral(s: string): number | null {
  if (/^\d+$/.test(s)) return Number(s);
  const digits: Record<string, number> = {
    一: 1,
    二: 2,
    两: 2,
    三: 3,
    四: 4,
    五: 5,
    六: 6,
    七: 7,
    八: 8,
    九: 9,
  };
  const tenIdx = s.indexOf('十');
  if (tenIdx === -1) return s.length === 1 ? (digits[s] ?? null) : null;
  const tens = tenIdx === 0 ? 1 : (digits[s[0]!] ?? null);
  const ones = tenIdx === s.length - 1 ? 0 : (digits[s.slice(tenIdx + 1)] ?? null);
  if (tens == null || ones == null) return null;
  return tens * 10 + ones;
}

/** 从标题里解析季数：「第三季」「第3季」都认，没有季标返回 null。 */
export function seasonFromTitle(title: string): number | null {
  const m = /第\s*([0-9一二两三四五六七八九十]+)\s*季/.exec(title);
  return m ? chineseNumeral(m[1]!) : null;
}

/** 挑下一季：同系列（带季角标）里大于当前季的最小已上线季。当前季数来源：
 *  works 自身条目角标 → 当前标题解析，都没有视作第 1 季接最小季；
 *  返回 null = 判不出，调用方走自己的回落。 */
export function pickNextSeason(
  works: RelatedItem[],
  currentSeriesId: string,
  currentTitle: string,
): RelatedItem | null {
  const tagged = works
    .map((item) => ({ item, no: seasonNoFromTag(item.tag) }))
    .filter(({ no, item }) => no != null && item.episodeCnt > 0)
    .sort((a, b) => a.no! - b.no!);
  if (tagged.length === 0) return null;

  const self = works.find((w) => w.seriesId === currentSeriesId);
  const currentNo = self ? seasonNoFromTag(self.tag) : (seasonFromTitle(currentTitle) ?? null);
  // 无标第一季：plan/v 只回其他季，剩下的带标季全是它的续季
  if (currentNo == null) return tagged[0]!.item;
  return tagged.find(({ no }) => no! > currentNo)?.item ?? null;
}

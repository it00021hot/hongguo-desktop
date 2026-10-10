//! 列表页标题过滤（历史/收藏/点赞/预约四页共用，对齐参考端 v1.1.6 的
//! 客户端搜索口径）。

/** query 为空直通；否则 title/seriesId contains（大小写不敏感）。 */
export function matchListQuery(query: string, title: string, seriesId: string): boolean {
  const q = query.trim().toLowerCase();
  if (q === '') return true;
  return title.toLowerCase().includes(q) || seriesId.toLowerCase().includes(q);
}

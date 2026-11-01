/**
 * 在播剧钉住（单一实现）：推荐流换一批/过期重开后列表内容整体更换，而
 * 游标是**数组下标**——在播剧没被钉在最前的话，游标 0 就指到别的剧，
 * 起点决策 effect 发现目标不一致立刻拽走播放目标，这正是「刷剧中被自动
 * 切到别的剧第 1 集」的根源。所以钉住必须**无条件**：只要闸开着且存在
 * 在播剧，就把它排到最前，不允许「流里已有就不钉」的旧短路——那条短路
 * 在新流还含着在播剧时恰好失守。
 *
 * 流里已有在播剧就原样复用那条（保留热度/角标等流条目信息，不降级成
 * 占位），缺失才调 `placeholderFor`（档案缓存拼占位，惰性：流里有就不花
 * 这次查询）。其余条目相对顺序不变，在播剧原位置滤掉不重复显示。
 */
export function withPinnedPlaying<T extends { seriesId: string }>(
  items: T[],
  pinHold: boolean,
  playingId: string | null,
  placeholderFor: () => T | null,
): T[] {
  if (!pinHold || !playingId) return items;
  const pin = items.find((i) => i.seriesId === playingId) ?? placeholderFor();
  if (!pin) return items;
  return [pin, ...items.filter((i) => i.seriesId !== pin.seriesId)];
}

/**
 * 在播剧钉住（单一实现）：游标是数组下标，在播剧不钉最前的话换一批/重开后游标 0 就指到别的剧，
 * 起点决策 effect 发现目标不一致会拽走播放目标（「刷剧中被自动切到别的剧第 1 集」的根源），
 * 所以闸开着就无条件钉最前，不允许「流里已有就不钉」的短路。流里已有在播剧就原样复用那条
 * （保留热度/角标等流条目信息），缺失才调 `placeholderFor` 拼占位；其余条目相对顺序不变。
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

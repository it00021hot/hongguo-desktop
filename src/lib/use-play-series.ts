import { useNavigate } from '@tanstack/react-router';

import { usePlayerStore } from '@/lib/stores/player';

/**
 * 「点卡片/联想词 → 直接播放」的统一路径：登记播放目标并跳播放页。
 *
 * 不预解析分集——播放页的 useSeriesEpisodes 自带回落解析与整页加载
 * 反馈（选集表/加载失败态都在那边），调用方只管表达意图。
 */
export function usePlaySeries() {
  const navigate = useNavigate();
  const setTarget = usePlayerStore((s) => s.setTarget);
  return (seriesId: string, vidIndex = 1) => {
    setTarget(seriesId, vidIndex);
    void navigate({ to: '/player' });
  };
}

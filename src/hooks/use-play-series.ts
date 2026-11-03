import { useLocation, useNavigate } from '@tanstack/react-router';

import { usePlayerStore } from '@/stores/player';

/**
 * 「点卡片/选定剧 → 直接播放」统一路径：returnTo 记来源路由（播放器返回钮回来源页），
 * requestGoto 点名目标并锁连播（选定剧进来滚轮/↑↓是切集），跳首页沉浸流播放。
 * vidIndex 不传 = 只点名剧（追更/收藏等不知道该看哪集的入口），起点由首页起点决策补上。
 */
export function usePlaySeries() {
  const navigate = useNavigate();
  const location = useLocation();
  const requestGoto = usePlayerStore((s) => s.requestGoto);
  const setBinge = usePlayerStore((s) => s.setBinge);
  const setReturnTo = usePlayerStore((s) => s.setReturnTo);
  return (seriesId: string, vidIndex?: number) => {
    setReturnTo(location.href);
    requestGoto(seriesId, vidIndex ?? 0);
    setBinge(seriesId);
    void navigate({ to: '/' });
  };
}

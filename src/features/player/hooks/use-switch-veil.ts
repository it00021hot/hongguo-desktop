import { useLayoutEffect, useRef, useState } from 'react';

/**
 * 切集黑幕（同剧切集的暗转遮罩状态）。
 *
 * 为什么要黑幕：切集间隙约 0.6-0.7s（取流 + `<video>` 重建 + 首帧解码，
 * 实测 622-746ms），旧「末帧定格」方案把画面静止钉满整个间隙——预期在
 * 动却突然不动，正是「切集像卡一下」的来源。黑幕在**切集提交的那一拍**
 * 就开始淡入（渲染层挂 transition-opacity），把静止期变成一个明确的转场
 * 动作：画面淡出 → 短暂黑场（顶部细进度条亮着）→ 新集首帧出画后黑幕
 * 淡出揭出已在播放的画面。全程没有「静止突变」，观感读作「翻页」。
 *
 * 只管状态与收口，显隐真值表在 use-frame-hold 的 overlayVisibility。
 * 切剧/初次起播不拉黑幕（封面接管 + 滑入动画语义不变）；清晰度切换
 * （episodeKey 不变）永不触发。
 */
export function useSwitchVeil(params: {
  seriesId: string | null;
  episodeKey: string;
  /** 播放侧真值（定义见 player-page）：新集画面已出画。 */
  videoLive: boolean;
  error: string | null;
  compatActive: boolean;
}) {
  const { seriesId, episodeKey, videoLive, error, compatActive } = params;
  /** 拉幕中的目标集（'' = 无幕）。收口即复位，同 key 再次切集仍能重新拉幕。 */
  const [veiledFor, setVeiledFor] = useState('');
  const prevTargetRef = useRef<{ seriesId: string | null; episodeKey: string }>({
    seriesId: null,
    episodeKey: '',
  });

  // episodeKey 变化的提交拍判型（useLayoutEffect：先于浏览器绘制，黑幕
  // 从用户看到的第一帧就在路上，不给静止末帧露脸的窗口）
  useLayoutEffect(() => {
    const prev = prevTargetRef.current;
    if (prev.episodeKey === episodeKey) return;
    prevTargetRef.current = { seriesId, episodeKey };
    // 只有同剧切集拉黑幕；切剧/初次起播交给封面接管
    if (seriesId && prev.seriesId === seriesId) setVeiledFor(episodeKey);
  }, [seriesId, episodeKey]);

  // 在幕 = 拉给当前集、且还没有收口信号（画面出画 / 取流失败 / 兜底转码）。
  // 渲染期派生（官方 adjust 模式）：条件在重渲染后自然为假，不循环。
  const veiled = veiledFor === episodeKey && !videoLive && !error && !compatActive;
  // 收口即复位（'' ），否则上一次切集的幕会误盖到后续的清晰度切换上。
  // 复位后 veiled 派生恒假，不会循环。
  if (veiledFor !== '' && !veiled) setVeiledFor('');

  return { veiled };
}

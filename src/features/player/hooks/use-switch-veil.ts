import { useLayoutEffect, useRef, useState } from 'react';

/**
 * 切集黑幕：把 0.6-0.7s 的切集静止间隙变成明确的暗转（淡出 → 黑场 → 揭出已在播放的画面），替代末帧定格的卡顿感。
 * 切集提交拍淡入；切剧/初次起播不拉幕（封面接管），清晰度切换永不触发。显隐真值表在 use-frame-hold。
 */
export function useSwitchVeil(params: {
  seriesId: string | null;
  episodeKey: string;
  /** 播放侧真值（定义见 player-page）：新集画面已出画。 */
  videoLive: boolean;
  error: string | null;
}) {
  const { seriesId, episodeKey, videoLive, error } = params;
  /** 拉幕中的目标集（'' = 无幕）。收口即复位，同 key 再次切集仍能重新拉幕。 */
  const [veiledFor, setVeiledFor] = useState('');
  const prevTargetRef = useRef<{ seriesId: string | null; episodeKey: string }>({
    seriesId: null,
    episodeKey: '',
  });

  // episodeKey 变化的提交拍判型（useLayoutEffect：黑幕从用户看到的第一帧就在路上）
  useLayoutEffect(() => {
    const prev = prevTargetRef.current;
    if (prev.episodeKey === episodeKey) return;
    prevTargetRef.current = { seriesId, episodeKey };
    // 只有同剧切集拉黑幕；切剧/初次起播交给封面接管
    if (seriesId && prev.seriesId === seriesId) setVeiledFor(episodeKey);
  }, [seriesId, episodeKey]);

  // 在幕 = 拉给当前集且未收口（画面出画/取流失败）；渲染期派生，条件在重渲染后自然为假不循环
  const veiled = veiledFor === episodeKey && !videoLive && !error;
  // 收口即复位，否则上一次切集的幕会误盖到后续的清晰度切换上
  if (veiledFor !== '' && !veiled) setVeiledFor('');

  return { veiled };
}

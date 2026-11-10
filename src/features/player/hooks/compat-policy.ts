/**
 * 兼容转码起播策略（纯函数）：何时主动转兼容、何时暂缓喂原生流。
 * Android 无 HEVC 扩展可装；WebView 软解判假后应立刻转兼容，而不是先黑屏再靠轮询兜底。
 */

export type CompatPlaybackDecision = {
  /** 本集应主动走兼容转码（不先试原生） */
  forceCompat: boolean;
  /** 暂不把原生地址喂给 `<video>`（等能力结论 / 等兼容产物） */
  holdNative: boolean;
};

/**
 * @param hevcPlayable 后端能力结论；`undefined` = 探测未返回
 * @param android Android WebView：软解不可播且无扩展安装路径
 * @param compatAuthorized 本会话用户已选「转兼容」（Windows 引导卡）
 */
export function decideCompatPlayback(input: {
  hevcPlayable: boolean | undefined;
  android: boolean;
  compatAuthorized: boolean;
  /** 后端标明本集必须转兼容（典型 bytevc2） */
  needsCompat?: boolean;
  directPlayback?: boolean;
}): CompatPlaybackDecision {
  if (input.directPlayback) return { forceCompat: false, holdNative: false };
  const { hevcPlayable, android, compatAuthorized, needsCompat } = input;
  const knownUnplayable = hevcPlayable === false;
  const forceCompat = Boolean(needsCompat) || (knownUnplayable && (android || compatAuthorized));
  // Android 能力未回前也不喂 HEVC：软解机会白下整集密文；硬解只多等一次廉价探测
  const holdNative = forceCompat || (android && hevcPlayable === undefined) || Boolean(needsCompat);
  return { forceCompat, holdNative };
}

/** 在 holdNative 且尚无兼容产物时，不把原生地址交给播放器。 */
export function resolvePlayBaseSrc(
  nativeSrc: string | null,
  compatSrc: string | null,
  holdNative: boolean,
): string | null {
  if (compatSrc) return compatSrc;
  if (holdNative) return null;
  return nativeSrc;
}

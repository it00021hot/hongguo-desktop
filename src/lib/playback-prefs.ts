/**
 * 播放偏好：倍速 / 音量 / 静音。
 *
 * 为什么必须持久化：切集、加载中、转兼容模式都会让 `<video>` 元素被卸载重建，
 * 新建元素的 `playbackRate`/`volume`/`muted` 一律回到默认值（1 / 1 / false）。
 * 所以「当前值」要存在本模块，「应用回元素」由播放器每次渲染后重放。
 *
 * 静音是独立状态：用户主动静音后 `volume` 仍是原值，不能用 `volume === 0`
 * 反推，否则恢复播放时会把静音悄悄取消。
 */

const RATE_KEY = 'hongguo.playbackRate';
const VOLUME_KEY = 'hongguo.volume';
const MUTED_KEY = 'hongguo.muted';

export const MIN_RATE = 0.25;
export const MAX_RATE = 4;

/** 读一个数值偏好；越界或损坏时回落到默认值。 */
function read(key: string, fallback: number, min: number, max: number): number {
  try {
    const v = parseFloat(window.localStorage.getItem(key) ?? '');
    return Number.isFinite(v) && v >= min && v <= max ? v : fallback;
  } catch {
    // 隐私模式等场景下 localStorage 会抛异常，此时用默认值即可
    return fallback;
  }
}

function write(key: string, value: number): void {
  try {
    window.localStorage.setItem(key, String(value));
  } catch {
    // 存不进去不影响本次会话内播放，下一次启动再退回默认值
  }
}

export const readPlaybackRate = (): number => read(RATE_KEY, 1, MIN_RATE, MAX_RATE);
export const writePlaybackRate = (rate: number): void => write(RATE_KEY, rate);

export const readVolume = (): number => read(VOLUME_KEY, 1, 0, 1);
export const writeVolume = (volume: number): void => write(VOLUME_KEY, volume);

export const readMuted = (): boolean => read(MUTED_KEY, 0, 0, 1) === 1;
export const writeMuted = (muted: boolean): void => write(MUTED_KEY, muted ? 1 : 0);

// ---------------------------------------------------------------- 弹幕开关

const DANMAKU_KEY = 'hongguo.danmaku';

/** 弹幕默认开。坏了或没存过都按开处理（多数人进来是想看弹幕的）。 */
export function readDanmakuEnabled(): boolean {
  try {
    const v = window.localStorage.getItem(DANMAKU_KEY);
    return v === null ? true : v === '1';
  } catch {
    return true;
  }
}

export function writeDanmakuEnabled(on: boolean): void {
  try {
    window.localStorage.setItem(DANMAKU_KEY, on ? '1' : '0');
  } catch {
    // 隐私模式下丢这一条无所谓，本次会话内开关仍然生效
  }
}

// ---------------------------------------------------------------- 弹幕显示设置

const DANMAKU_OPACITY_KEY = 'hongguo.danmakuOpacity';
const DANMAKU_FONT_KEY = 'hongguo.danmakuFontScale';
const DANMAKU_DENSITY_KEY = 'hongguo.danmakuDensity';
const DANMAKU_AREA_KEY = 'hongguo.danmakuArea';

/** 弹幕显示设置（透明度 / 字号 / 密度 / 显示区域），作用于渲染层。 */
export interface DanmakuDisplaySettings {
  /** 文字不透明度 0.1–1 */
  opacity: number;
  /** 字号缩放 0.5–2（1 = 按画面高度自适应的基准字号） */
  fontScale: number;
  /** 密度 0–1（1 = 全部显示，入场时按比例丢弃） */
  density: number;
  /** 显示区域 0.25–1（弹幕占据画面顶部的高度比例） */
  area: number;
}

export const DANMAKU_DEFAULTS: DanmakuDisplaySettings = {
  opacity: 0.9,
  fontScale: 1,
  density: 1,
  area: 1,
};

export function readDanmakuDisplay(): DanmakuDisplaySettings {
  return {
    opacity: read(DANMAKU_OPACITY_KEY, DANMAKU_DEFAULTS.opacity, 0.1, 1),
    fontScale: read(DANMAKU_FONT_KEY, DANMAKU_DEFAULTS.fontScale, 0.5, 2),
    density: read(DANMAKU_DENSITY_KEY, DANMAKU_DEFAULTS.density, 0, 1),
    area: read(DANMAKU_AREA_KEY, DANMAKU_DEFAULTS.area, 0.25, 1),
  };
}

export function writeDanmakuDisplay(s: DanmakuDisplaySettings): void {
  write(DANMAKU_OPACITY_KEY, s.opacity);
  write(DANMAKU_FONT_KEY, s.fontScale);
  write(DANMAKU_DENSITY_KEY, s.density);
  write(DANMAKU_AREA_KEY, s.area);
}

// ---------------------------------------------------------------- 隐身模式

const INCOGNITO_KEY = 'hongguo.incognito';

/** 隐身模式默认关：开启后鼠标离开窗口即整窗透明 + 暂停。 */
export function readIncognito(): boolean {
  try {
    return window.localStorage.getItem(INCOGNITO_KEY) === '1';
  } catch {
    return false;
  }
}

export function writeIncognito(on: boolean): void {
  try {
    window.localStorage.setItem(INCOGNITO_KEY, on ? '1' : '0');
  } catch {
    // 存不进去只影响下次启动的初始状态，本次会话内开关照常生效
  }
}

// ---------------------------------------------------------------- 上次播放目标

const LAST_TARGET_KEY = 'hongguo.lastTarget';

export interface LastPlayTarget {
  seriesId: string;
  vidIndex: number;
}

/**
 * 上次播放目标，供刷新/重启后恢复播放器（进度本身由本地播放档案的
 * resumeAt 保证，这里只记「在看哪部哪集」）。
 */
export function readLastTarget(): LastPlayTarget | null {
  try {
    const raw = window.localStorage.getItem(LAST_TARGET_KEY);
    if (!raw) return null;
    const v = JSON.parse(raw);
    return typeof v?.seriesId === 'string' && v.seriesId !== '' && Number.isFinite(v?.vidIndex)
      ? { seriesId: v.seriesId, vidIndex: v.vidIndex }
      : null;
  } catch {
    return null;
  }
}

export function writeLastTarget(target: LastPlayTarget | null): void {
  try {
    if (target) window.localStorage.setItem(LAST_TARGET_KEY, JSON.stringify(target));
    else window.localStorage.removeItem(LAST_TARGET_KEY);
  } catch {
    // 恢复不了只是回到空态，不影响播放
  }
}

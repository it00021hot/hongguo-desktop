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

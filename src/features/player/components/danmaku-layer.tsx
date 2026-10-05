import { useEffect, useRef } from 'react';
import type { DanmakuDisplaySettings } from '@/lib/playback-prefs';
import type { Danmaku } from '@/lib/schema';

interface Props {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  items: Danmaku[];
  enabled: boolean;
  /** 透明度 / 字号 / 密度 / 显示区域（用户可在弹幕设置面板里调） */
  display: DanmakuDisplaySettings;
}

/** 一发已入场的弹幕：按视频时间轴定位，暂停自动冻结。 */
interface Shot {
  text: string;
  lane: number;
  spawnAt: number; // 视频内秒
  width: number; // 量好的文本宽
}

/** 滚过一屏的时长（秒）：太快要看不清，太慢会叠 lane。 */
const CROSS_SECONDS = 9;
/** lane 数量按高度自适应的下限/上限。 */
const MIN_LANES = 4;
const FONT_MIN = 15;
const FONT_MAX = 26;

/**
 * 弹幕渲染层：canvas 覆盖在 `<video>` 上，rAF 驱动。
 *
 * 定位完全锚在 `video.currentTime` 上——暂停即冻结、倍速自然加速，
 * 不需要单独监听 play/pause/ratechange。seek 由时间跳变检测兜底：
 * 重建 spawn 游标并清空已入场弹幕（旧位置的残留没有意义）。
 */
export function DanmakuLayer({ videoRef, items, enabled, display }: Props) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  // rAF 闭包要读最新 items/enabled/display，用 ref 镜像避免反复重启循环
  const itemsRef = useRef(items);
  const enabledRef = useRef(enabled);
  const displayRef = useRef(display);
  /** resize 依赖 display（字号/显示区域），由 effect 触发重量 */
  const resizeRef = useRef<() => void>(() => {});
  useEffect(() => {
    itemsRef.current = items;
    enabledRef.current = enabled;
    displayRef.current = display;
  });
  useEffect(() => {
    resizeRef.current();
  }, [display]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const container = canvas?.parentElement;
    const video = videoRef.current;
    if (!canvas || !container || !video) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    let width = 0;
    let height = 0;
    let fontPx = 18;
    let lanes = MIN_LANES;
    /** 弹幕实际占据的高度（显示区域比例），lane 只分布在这一段 */
    let danmakuHeight = 0;
    let raf = 0;
    /** 弹幕源按 offsetMs 升序（后端已排），spawned 是「已入场」游标 */
    let spawned = 0;
    let shots: Shot[] = [];
    /** 每 lane 的「最早可再投放」视频时刻 */
    let laneFreeAt: number[] = [];
    let lastTime = video.currentTime;

    const resize = () => {
      const rect = container.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      const settings = displayRef.current;
      width = rect.width;
      height = rect.height;
      danmakuHeight = Math.round(height * settings.area);
      const base = Math.min(FONT_MAX, Math.max(FONT_MIN, height / 22));
      fontPx = Math.max(10, Math.round(base * settings.fontScale));
      lanes = Math.max(MIN_LANES, Math.floor(danmakuHeight / (fontPx * 1.9)));
      laneFreeAt = new Array(lanes).fill(0);
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.font = `bold ${fontPx}px system-ui, "Microsoft YaHei", sans-serif`;
      ctx.textBaseline = 'middle';
    };
    resize();
    resizeRef.current = resize;
    const ro = new ResizeObserver(resize);
    ro.observe(container);

    const resetCursor = (t: number) => {
      // 二分找第一个 offset > t 的位置：之前的都视为已过（seek 跳过）
      const ms = t * 1000;
      let lo = 0;
      let hi = itemsRef.current.length;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if ((itemsRef.current[mid]?.offsetMs ?? 0) <= ms) lo = mid + 1;
        else hi = mid;
      }
      spawned = lo;
      shots = [];
      laneFreeAt = new Array(lanes).fill(0);
    };

    const tick = () => {
      raf = requestAnimationFrame(tick);
      const t = video.currentTime;
      const src = itemsRef.current;
      const settings = displayRef.current;

      // 时间倒跳或大幅前跳 = seek：重置游标（小抖动忽略）
      if (t < lastTime - 0.05 || t > lastTime + 1.5) resetCursor(t);
      lastTime = t;

      if (enabledRef.current && src.length > 0) {
        // 入场：offset 落在 (上一帧, 当前] 的弹幕全部发射
        for (;;) {
          const d = src[spawned];
          if (!d || d.offsetMs / 1000 > t) break;
          spawned += 1;
          if (!d.text) continue;
          // 密度：按显示比例丢弃（随机均匀，不做确定性抽样——
          // 丢的是「哪一条」无所谓，要的是「同屏条数按比例变稀」）
          if (Math.random() > settings.density) continue;
          const textWidth = ctx.measureText(d.text).width;
          // 选一条「右端已让出足够空隙」的 lane；都不空就叠到最空那条
          let pick = 0;
          let earliest = Number.POSITIVE_INFINITY;
          let placed = false;
          for (let i = 0; i < lanes; i += 1) {
            const freeAt = laneFreeAt[i] ?? 0;
            if (freeAt <= t) {
              pick = i;
              placed = true;
              break;
            }
            if (freeAt < earliest) {
              earliest = freeAt;
              pick = i;
            }
          }
          if (!placed) pick = Math.floor(Math.random() * lanes);
          // 该 lane 再投放的最早时刻：本条完全进入屏幕 + 半个身位
          const enterSeconds = (textWidth / width) * CROSS_SECONDS + 0.5;
          laneFreeAt[pick] = t + enterSeconds;
          shots.push({ text: d.text, lane: pick, spawnAt: t, width: textWidth });
        }
      }

      // 绘制
      ctx.clearRect(0, 0, width, height);
      if (!enabledRef.current) return;
      const speed = width / CROSS_SECONDS;
      const laneHeight = danmakuHeight / lanes;
      ctx.globalAlpha = settings.opacity;
      shots = shots.filter((s) => {
        const age = t - s.spawnAt;
        const x = width - age * speed;
        if (x + s.width < 0) return false;
        const y = s.lane * laneHeight + laneHeight / 2;
        ctx.lineWidth = 2;
        ctx.strokeStyle = 'rgba(0,0,0,0.75)';
        ctx.strokeText(s.text, x, y);
        ctx.fillStyle = '#ffffff';
        ctx.fillText(s.text, x, y);
        return true;
      });
      ctx.globalAlpha = 1;
    };
    raf = requestAnimationFrame(tick);

    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
  }, [videoRef]);

  // 不能按 enabled 卸载 canvas：effect 依赖只有 videoRef，卸载后重挂
  // 拿到的是新元素而 effect 不会重跑，弹幕就永远黑了。用 CSS 收起来。
  return (
    <canvas
      ref={canvasRef}
      className={`pointer-events-none absolute inset-0 z-10 ${enabled ? '' : 'invisible'}`}
      aria-hidden
    />
  );
}

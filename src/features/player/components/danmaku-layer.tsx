import { useEffect, useRef } from 'react';
import { parseEmojiSegments } from '@/lib/danmaku-emoji';
import type { DanmakuDisplaySettings } from '@/lib/playback-prefs';
import type { Danmaku } from '@/lib/schema';

interface Props {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  items: Danmaku[];
  enabled: boolean;
  /** 透明度 / 字号 / 密度 / 显示区域（用户可在弹幕设置面板里调） */
  display: DanmakuDisplaySettings;
}

/** 滚过一屏的时长（毫秒）：hgplayer durationMs 默认 9000。 */
const CROSS_MS = 9000;
/** 同一泳道相邻两条的入场间隔（视频秒）：hgplayer 占位 +1400ms 同款。 */
const LANE_BUSY_S = 1.4;
/** 泳道高 = 字号 + 10（hgplayer top 计算同款）。 */
const LANE_GAP = 10;
/** lane 数量按高度自适应的下限。 */
const MIN_LANES = 4;
const FONT_MIN = 15;
const FONT_MAX = 26;

/**
 * 弹幕渲染层（对齐 hgplayer 的 DOM 形态，替代旧 canvas）：
 *
 * 每条弹幕是一个绝对定位的 `<span>`，Web Animations API 线性滚过一屏，
 * 位置更新交给浏览器合成器，rAF 只负责按 `video.currentTime` 入场——
 * 暂停即冻结（动画 pause）、倍速同步（动画 playbackRate）、seek 由时间
 * 跳变检测兜底（清屏重建入场游标）。
 *
 * 文本排版走 CSS：600 字重 + 柔和投影（`0 1px 3px rgba(0,0,0,.9)`，
 * hgplayer `.dm` 同款）替代旧实现的 2px 硬描边——描边是「弹幕刺眼」
 * 的根源。`[名字]` 表情代码按映射表换成打包的 webp 图。
 */
export function DanmakuLayer({ videoRef, items, enabled, display }: Props) {
  const layerRef = useRef<HTMLDivElement | null>(null);
  // rAF 闭包要读最新 items/enabled/display，用 ref 镜像避免反复重启循环
  const itemsRef = useRef(items);
  const enabledRef = useRef(enabled);
  const displayRef = useRef(display);
  /** resize / 开关清屏依赖 props，由 effect 触发闭包里的最新实现 */
  const resizeRef = useRef<() => void>(() => {});
  const resetRef = useRef<() => void>(() => {});
  useEffect(() => {
    itemsRef.current = items;
    enabledRef.current = enabled;
    displayRef.current = display;
  });
  useEffect(() => {
    resizeRef.current();
  }, [display]);
  // 开关弹幕：清掉在飞的旧弹幕、入场游标落到当前时间（不是把积压的一股脑放出）
  useEffect(() => {
    resetRef.current();
  }, [enabled]);

  useEffect(() => {
    const layer = layerRef.current;
    if (!layer) return;

    let width = 0;
    let height = 0;
    let fontPx = 18;
    let lanes = MIN_LANES;
    let raf = 0;
    /** 弹幕源按 offsetMs 升序（后端已排），spawned 是「已入场」游标 */
    let spawned = 0;
    /** 在飞的动画；暂停/倍速/清理要整体操作 */
    let flights: Animation[] = [];
    /** 每条泳道「最早可再入场」的视频时刻 */
    let laneFreeAt: number[] = [];
    /**
     * 当前绑定的 video 元素。player-page 里「切集 / 加载中都会让
     * `<video>` 被卸载重建」（重取流时 src 撤下再挂回），元素会被换成
     * 新实例——effect 只跑一次，不能在挂载时抓死，tick 里每帧从 ref
     * 现取、变了就重绑（重置游标 + 清屏）。
     */
    let boundVideo: HTMLVideoElement | null = null;
    let lastTime = 0;
    let lastPaused = false;
    let lastRate = 1;

    const resize = () => {
      const rect = layer.getBoundingClientRect();
      const settings = displayRef.current;
      width = rect.width;
      height = rect.height;
      const base = Math.min(FONT_MAX, Math.max(FONT_MIN, height / 22));
      fontPx = Math.max(10, Math.round(base * settings.fontScale));
      // 泳道只分布在显示区域内（area = 占画面高度的比例）
      lanes = Math.max(MIN_LANES, Math.floor((height * settings.area) / (fontPx + LANE_GAP)));
      laneFreeAt = new Array(lanes).fill(0);
    };

    const clearFlights = () => {
      for (const a of flights) a.cancel();
      flights = [];
      layer.replaceChildren();
    };

    // 二分找第一个 offset > t 的位置：之前的都视为已过（seek 跳过）
    const resetCursor = (t: number) => {
      const ms = t * 1000;
      let lo = 0;
      let hi = itemsRef.current.length;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if ((itemsRef.current[mid]?.offsetMs ?? 0) <= ms) lo = mid + 1;
        else hi = mid;
      }
      spawned = lo;
      laneFreeAt = new Array(lanes).fill(0);
      clearFlights();
    };

    /** 组一条弹幕的内容：文本段 + `[名字]` 表情图（hgplayer 排版同款）。 */
    const appendContent = (el: HTMLSpanElement, text: string) => {
      for (const seg of parseEmojiSegments(text)) {
        if (seg.kind === 'text') {
          el.appendChild(document.createTextNode(seg.value));
        } else {
          const img = document.createElement('img');
          img.src = seg.url;
          img.alt = seg.value;
          img.draggable = false;
          // 1.3em 见方、基线下沉 0.28em（hgplayer .dm-emo 同款）
          img.style.cssText =
            'width:1.3em;height:1.3em;margin:0 1px;vertical-align:-0.28em;object-fit:contain';
          el.appendChild(img);
        }
      }
    };

    const fire = (text: string, t: number, video: HTMLVideoElement) => {
      const el = document.createElement('span');
      // hgplayer .dm 同款：600 字重 + 柔和投影，不用描边
      el.style.cssText =
        'position:absolute;left:0;white-space:nowrap;font-weight:600;color:#fff;' +
        `font-size:${fontPx}px;text-shadow:0 1px 3px rgba(0,0,0,.9);will-change:transform;`;
      appendContent(el, text);
      // 泳道：挑最早空出来的那条并占位（hgplayer 同款，叠满时自然排队）
      let pick = 0;
      for (let i = 1; i < lanes; i += 1) {
        if ((laneFreeAt[i] ?? 0) < (laneFreeAt[pick] ?? 0)) pick = i;
      }
      laneFreeAt[pick] = t + LANE_BUSY_S;
      el.style.top = `${pick * (fontPx + LANE_GAP)}px`;
      layer.appendChild(el);

      const anim = el.animate(
        [
          { transform: `translateX(${width}px)` },
          { transform: `translateX(-${el.offsetWidth}px)` },
        ],
        { duration: CROSS_MS, easing: 'linear', fill: 'forwards' },
      );
      // 倍速联动 + 暂停冻结（旧 canvas 实现锚视频时间轴，天然同步）
      anim.playbackRate = video.playbackRate;
      if (video.paused) anim.pause();
      anim.onfinish = () => {
        el.remove();
        flights = flights.filter((a) => a !== anim);
      };
      flights.push(anim);
    };

    const tick = () => {
      raf = requestAnimationFrame(tick);
      // 每帧现取 video：换元素（切集/重取流）即重绑
      const video = videoRef.current;
      if (video !== boundVideo) {
        boundVideo = video;
        if (video) {
          lastTime = video.currentTime;
          lastPaused = video.paused;
          lastRate = video.playbackRate;
          resetCursor(lastTime);
        }
        return;
      }
      if (!video) return;

      const t = video.currentTime;
      const src = itemsRef.current;

      // 时间倒跳或大幅前跳 = seek：重置游标（小抖动忽略）
      if (t < lastTime - 0.05 || t > lastTime + 1.5) resetCursor(t);
      lastTime = t;
      // 播放态/倍速漂移检测（不挂事件：元素随时可能被换掉）
      if (video.paused !== lastPaused) {
        lastPaused = video.paused;
        for (const a of flights) {
          if (video.paused) a.pause();
          else a.play();
        }
      }
      if (video.playbackRate !== lastRate) {
        lastRate = video.playbackRate;
        for (const a of flights) a.updatePlaybackRate(video.playbackRate);
      }
      if (!enabledRef.current) return;

      // 入场：offset 落在 (上一帧, 当前] 的弹幕全部发射
      for (;;) {
        const d = src[spawned];
        if (!d || d.offsetMs / 1000 > t) break;
        spawned += 1;
        if (!d.text) continue;
        // 密度：按显示比例丢弃（随机均匀，不做确定性抽样——
        // 丢的是「哪一条」无所谓，要的是「同屏条数按比例变稀」）
        if (Math.random() > displayRef.current.density) continue;
        fire(d.text, t, video);
      }
    };

    resize();
    resizeRef.current = resize;
    resetRef.current = () => {
      const video = videoRef.current;
      if (video) resetCursor(video.currentTime);
    };
    const ro = new ResizeObserver(resize);
    ro.observe(layer);
    raf = requestAnimationFrame(tick);

    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      clearFlights();
    };
  }, [videoRef]);

  // 不能按 enabled 卸载容器：effect 依赖只有 videoRef，卸载后重挂
  // 拿到的是新元素而 effect 不会重跑，弹幕就永远黑了。用 CSS 收起来。
  return (
    <div
      ref={layerRef}
      className={`pointer-events-none absolute inset-0 z-10 overflow-hidden ${enabled ? '' : 'invisible'}`}
      style={{ opacity: display.opacity }}
      aria-hidden
    />
  );
}

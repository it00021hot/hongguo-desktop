/**
 * 画质增强叠加层：Anime4K（WebGPU canvas 盖在视频上）与锐化两个独立
 * 开关的运行时宿主。
 *
 * - 只开锐化：SVG 卷积滤镜打到 `<video>` 上（纯合成器效果，全平台可用）；
 * - 开 Anime4K：WebGPU canvas 盖在视频上，锐化（若同开）直接进显示
 *   着色器、对 2× 输出做 unsharp——开关切换与拖滑条都只写 uniform，
 *   不重建渲染器。
 *
 * 显示层增强——不碰播放管线，任何失败只收掉增强画面本身，播放不受
 * 影响；每条失败路径都给人话 + 下一步。canvas 尺寸 = 增强输出分辨率，
 * 适配交给 object-fit，与 `<video>` 同一套裁切口径（小屏 cover / 其余 contain）。
 */
import { useEffect, useId, useRef } from 'react';
import type { Dispatch, RefObject, SetStateAction } from 'react';
import {
  createAnime4KRenderer,
  EnhancementError,
  sharpenKernel,
  type Anime4KRenderer,
  type EnhancementErrorCode,
} from '../enhancement';
import type { EnhancementStatus } from '../hooks/use-enhancement';
import type { EnhancementSettings } from '@/utils/playback-prefs';
import { t } from '@/locales';
import { cn } from '@/lib/utils';
import type { MessageKey } from '@/locales';

/** 失败码 → 说明词条。每条路径都要说明「发生了什么 + 已退回原画 + 替代档」。 */
const ERROR_MESSAGE_KEYS: Record<EnhancementErrorCode, MessageKey> = {
  noWebgpu: 'player.enhanceErrNoWebgpu',
  noAdapter: 'player.enhanceErrNoAdapter',
  gpuUnsupported: 'player.enhanceErrGpuUnsupported',
  inputTooLarge: 'player.enhanceErrTooLarge',
  canvasUnavailable: 'player.enhanceErrGeneric',
  slow: 'player.enhanceErrSlow',
  restricted: 'player.enhanceErrRestricted',
  sizeChanged: 'player.enhanceErrSizeChanged',
  notReady: 'player.enhanceErrNotReady',
  generic: 'player.enhanceErrGeneric',
};

interface Props {
  videoRef: RefObject<HTMLVideoElement | null>;
  /**
   * 播放地址。`<video>` 以它为 key 重建，本层 effect 跟着重建——渲染器
   * 绑的是旧元素，不重建就会一直读已卸载元素的画面。
   */
  src: string | null;
  /** 画面可用（有流且已出画）才允许增强；false 时层保持空转 */
  available: boolean;
  /** 小屏 cover：与 <video> 的 object-fit 保持一致，画布才对得上画面 */
  cover: boolean;
  settings: EnhancementSettings;
  onStatus: Dispatch<SetStateAction<EnhancementStatus>>;
}

export function EnhancementLayer({ videoRef, src, available, cover, settings, onStatus }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  /** 活着的 Anime4K 渲染器（若开）：强度变化经它直写 uniform，不重建 */
  const rendererRef = useRef<Anime4KRenderer | null>(null);
  const filterId = `video-sharpen-${useId().replace(/:/g, '')}`;

  // Anime4K 生命周期：只在开/关与换源时启停。渲染器自己管 canvas 显隐
  // （首帧成功才翻 visible）与状态上报；SVG 滤镜归下一个 effect 管。
  useEffect(() => {
    const video = videoRef.current;
    const canvas = canvasRef.current;
    if (!video || !canvas) return;
    canvas.style.visibility = 'hidden';
    onStatus({ state: 'idle', message: '' });
    if (!available || !src || !settings.anime4kOn) return;

    // WebGPU 渲染循环。rVFC 对齐解码帧节奏（旧 WKWebView 无此 API，回落
    // rAF）；可见性/暂停/seek 都不画，省电且避免错帧
    const abort = new AbortController();
    let renderer: Anime4KRenderer | null = null;
    let pending = false;
    let failed = false;
    let frameId: number | null = null;
    let rafId: number | null = null;
    let drawing = false;
    let redraw = false;
    let revision = 0;
    const cancelFrame = () => {
      if (frameId !== null) video.cancelVideoFrameCallback(frameId);
      if (rafId !== null) cancelAnimationFrame(rafId);
      frameId = rafId = null;
    };
    const fail = (error: unknown) => {
      if (abort.signal.aborted || failed) return;
      failed = true;
      cancelFrame();
      canvas.style.visibility = 'hidden';
      renderer?.dispose();
      rendererRef.current = null;
      // 根因留给排查通道，用户看到的只有映射好的词条
      console.warn('[enhancement] anime4k 已退回原画', error);
      const code = error instanceof EnhancementError ? error.code : 'generic';
      onStatus({ state: 'error', message: t(ERROR_MESSAGE_KEYS[code]) });
    };
    const draw = async () => {
      if (abort.signal.aborted || failed) return;
      if (!renderer) {
        await initialize();
        return;
      }
      // 单飞 + redraw 锁存：上一帧还在 GPU 队列里时不叠加排队，只补一帧
      if (drawing) {
        redraw = true;
        return;
      }
      drawing = true;
      const drawRevision = revision;
      try {
        const rendered = await renderer.draw();
        if (!abort.signal.aborted && !failed && drawRevision === revision && rendered) {
          canvas.style.visibility = 'visible';
          onStatus((prev) =>
            prev.state === 'ready'
              ? prev
              : { state: 'ready', message: t('player.enhanceAnime4kOn') },
          );
        }
      } catch (error) {
        fail(error);
      } finally {
        drawing = false;
        if (redraw) {
          redraw = false;
          void draw();
        } else {
          schedule();
        }
      }
    };
    const schedule = () => {
      cancelFrame();
      if (
        abort.signal.aborted ||
        failed ||
        document.hidden ||
        video.paused ||
        video.ended ||
        video.seeking
      ) {
        return;
      }
      if ('requestVideoFrameCallback' in video) {
        frameId = video.requestVideoFrameCallback(() => {
          frameId = null;
          void draw();
        });
      } else {
        rafId = requestAnimationFrame(() => {
          rafId = null;
          void draw();
        });
      }
    };
    const initialize = async () => {
      if (
        pending ||
        renderer ||
        failed ||
        abort.signal.aborted ||
        video.readyState < 2 ||
        !video.videoWidth
      ) {
        return;
      }
      pending = true;
      onStatus({ state: 'loading', message: t('player.enhancePreparing') });
      try {
        const next = await createAnime4KRenderer(video, canvas, {
          signal: abort.signal,
          onError: fail,
          sharpen: settings.sharpenOn ? settings.strength : 0,
        });
        if (abort.signal.aborted || failed) {
          next.dispose();
          return;
        }
        renderer = next;
        rendererRef.current = next;
        void draw();
      } catch (error) {
        // abort 引发的失败由 cleanup 收口，不算增强失败
        if (!(error instanceof DOMException && error.name === 'AbortError')) fail(error);
      } finally {
        pending = false;
      }
    };
    // seek/换流时旧帧立即隐掉，别让增强画面停在过去的画面上
    const invalidate = () => {
      revision++;
      cancelFrame();
      canvas.style.visibility = 'hidden';
    };
    const refresh = () => {
      cancelFrame();
      void draw();
    };
    const visibilityChange = () => {
      if (document.hidden) invalidate();
      else refresh();
    };
    const events = ['loadeddata', 'seeked', 'play', 'pause', 'ended', 'resize'] as const;
    events.forEach((event) => video.addEventListener(event, refresh));
    video.addEventListener('seeking', invalidate);
    video.addEventListener('emptied', invalidate);
    document.addEventListener('visibilitychange', visibilityChange);
    void initialize();
    return () => {
      abort.abort();
      cancelFrame();
      renderer?.dispose();
      rendererRef.current = null;
      canvas.style.visibility = 'hidden';
      events.forEach((event) => video.removeEventListener(event, refresh));
      video.removeEventListener('seeking', invalidate);
      video.removeEventListener('emptied', invalidate);
      document.removeEventListener('visibilitychange', visibilityChange);
    };
    // 初始锐化值随建随用；开关/力度的后续变化走 setSharpen effect 直写
    // uniform（刻意不进依赖——进来了每次拖滑条都重建渲染器白吃一次 warm-up）
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [videoRef, src, available, settings.anime4kOn, filterId, onStatus]);

  // 纯 SVG 锐化（Anime4K 关时）：滤镜打到 <video> 上。Anime4K 开着时
  // 锐化走显示着色器，这条路径必须让位（双重锐化=振铃）。
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const active = available && settings.sharpenOn && !settings.anime4kOn;
    const originalFilter = video.style.filter;
    if (active) {
      video.style.filter = `url(#${filterId})`;
      onStatus({ state: 'ready', message: t('player.enhanceSharpenOn') });
    } else if (!settings.anime4kOn) {
      // 只在 Anime4K 关时接管状态行——开着时状态归渲染循环
      onStatus({ state: 'idle', message: '' });
    }
    return () => {
      video.style.filter = originalFilter;
    };
  }, [videoRef, available, settings.sharpenOn, settings.anime4kOn, filterId, onStatus]);

  // 强度/锐化开关变化直写 uniform：渲染器若活着立刻生效，拖滑条不闪不断
  useEffect(() => {
    rendererRef.current?.setSharpen(settings.sharpenOn ? settings.strength : 0);
  }, [settings.sharpenOn, settings.strength]);

  return (
    <>
      {/* 锐化核矩阵定义（仅 SVG 路径消费）。size-0 藏形，url(#id) 引用照常生效 */}
      <svg aria-hidden focusable="false" className="pointer-events-none absolute size-0">
        <defs>
          <filter
            id={filterId}
            x="0"
            y="0"
            width="100%"
            height="100%"
            colorInterpolationFilters="sRGB"
          >
            <feConvolveMatrix
              order="3"
              kernelMatrix={sharpenKernel(settings.strength).join(' ')}
              divisor="1"
              bias="0"
              edgeMode="duplicate"
              preserveAlpha="true"
            />
          </filter>
        </defs>
      </svg>
      {/* z-[5]：盖过视频本体、让位中央 loading（z-[6]）与弹幕层（z-10）。
          首帧渲染成功前保持不可见（draw 循环里翻 visibility） */}
      <canvas
        ref={canvasRef}
        aria-hidden
        className={cn(
          'pointer-events-none invisible absolute inset-0 z-[5] size-full',
          cover ? 'object-cover' : 'object-contain',
        )}
      />
    </>
  );
}

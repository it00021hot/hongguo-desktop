import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Switch } from '@/components/ui/switch';
import { Progress } from '@/components/ui/progress';
import { Label } from '@/components/ui/label';
import { Card } from '@/components/ui/card';
import { SeriesPanel } from './series-panel';
import { DanmakuLayer } from './danmaku-layer';
import { PlayerControls } from './player-controls';
import { ContinueWatching } from './continue-watching';
import {
  usePlay,
  useSavePosition,
  useSaveSettings,
  useCompatPlayback,
  useDanmaku,
  useSeriesEpisodes,
  useSettings,
  useStorageActions,
} from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import {
  readDanmakuEnabled,
  readMuted,
  readPlaybackRate,
  readVolume,
  writeDanmakuEnabled,
  writeMuted,
  writePlaybackRate,
  writeVolume,
} from '@/lib/playback-prefs';
import { t, tf } from '@/i18n';
import { useEvent } from '@/lib/ipc/events';
import { EVENTS } from '@/lib/ipc/types';
import { formatBytes } from '@/lib/format';
import type { CompatProgress, OnlineProgress, Settings, VideoDefinition } from '@/lib/schema';

/** 进度保存间隔（毫秒）。太频繁会写爆磁盘，太稀疏丢进度。 */
const SAVE_INTERVAL = 5_000;

export function PlayerPage() {
  const seriesId = usePlayerStore((s) => s.seriesId);
  const vidIndex = usePlayerStore((s) => s.vidIndex);

  // 没在播时主区域就是「继续观看」——播放历史的入口必须在这里，
  // 放进只在播放时才渲染的侧栏等于没做。
  if (!seriesId || !vidIndex) {
    return <ContinueWatching />;
  }

  // key 随剧集变化 → 切集时组件整体重建，播放/转码状态自然清零，
  // 不必在 effect 里同步 setState（那会触发级联渲染）。
  return <PlayerView key={`${seriesId}:${vidIndex}`} />;
}

function PlayerView() {
  const videoRef = useRef<HTMLVideoElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const lastSaved = useRef(0);
  const pendingSeek = useRef(0);
  /**
   * 最近一次的播放位置与时长，无论有没有真的落盘。
   *
   * 卸载时 `<video>` 已经先一步被卸载，从元素上再读 currentTime 是拿不到的；
   * 内存里这份是唯一能在 cleanup 里用到的真值。
   *
   * `key` 记的是这份位置属于哪一集：组件按剧集整体重挂载，但 ref 不会重置，
   * 不带 key 的话新一集会拿着上一集的秒数去续播。
   */
  const lastKnown = useRef({ key: '', time: 0, duration: 0 });

  // 切集 / 加载中都会让 <video> 被卸载重建，新元素的倍速音量静音
  // 一律回到默认值，所以「用户设的值」要存在这里，每次渲染后再贴回元素。
  const playbackRateRef = useRef(readPlaybackRate());
  const volumeRef = useRef(readVolume());
  const mutedRef = useRef(readMuted());

  const seriesId = usePlayerStore((s) => s.seriesId);
  const vidIndex = usePlayerStore((s) => s.vidIndex);
  const setTarget = usePlayerStore((s) => s.setTarget);
  const episodeKey = seriesId && vidIndex ? `${seriesId}:${vidIndex}` : '';

  const { data: settings, isPending: settingsPending } = useSettings();
  const { mutate: saveSettings } = useSaveSettings();
  const { deleteEpisode } = useStorageActions();
  // 设置没加载完时先显示 false，但开关同时锁住：否则用户会在这个窗口里
  // 拨动开关，把默认值当成后端真值写回去。
  const autoNext = settings?.autoNextEpisode ?? false;
  const autoDelete = settings?.autoDeleteAfterPlay ?? false;
  const patchSettings = (next: Partial<Settings>) => {
    if (settings) saveSettings({ ...settings, ...next });
  };

  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** 下载面板是否打开。开着时不让连播把这一集换掉。 */
  const [downloading, setDownloading] = useState(false);
  /**
   * 在线取流/解密的进度。
   *
   * 整集取回 + 解密要等一会儿，这段时间里界面上原本只有一个转圈。
   * 播未下载的集时这里显示「正在缓存 45%（54/120MB）」，用户才知道它在动。
   */
  const [buffering, setBuffering] = useState<OnlineProgress | null>(null);
  /**
   * 用户选的清晰度。`undefined` = 不指定，由后端取平台给的最高档。
   *
   * 存在组件里而不是 store：切集时应当回到「自动最高档」，
   * 不同集提供的档位本来就不一样，把上一集的档位带过去多半要触发回退。
   */
  const [definition, setDefinition] = useState<number | undefined>(undefined);
  /** 本集实际生效的档位与全部可选档位，由起播响应带回。 */
  const [activeDefinition, setActiveDefinition] = useState(0);
  const [definitions, setDefinitions] = useState<VideoDefinition[]>([]);

  const { mutate: play } = usePlay();
  const { mutate: savePosition } = useSavePosition();
  const compatPlay = useCompatPlayback();

  // 在线取流进度：只认当前这一集，换集后清掉。
  useEvent<OnlineProgress>(
    EVENTS.onlinePlayProgress,
    useCallback(
      (p: OnlineProgress) => {
        setBuffering(p.key === episodeKey ? p : null);
      },
      [episodeKey],
    ),
  );
  // 选集与「下载到本地」都要完整分集表，从剧集档案直接取
  const { data: currentSeries } = useSeriesEpisodes(seriesId);

  // 弹幕：vid 来自剧集档案的分集表，换集自动换一份缓存
  const currentVid = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex)?.vid ?? '';
  const danmakuQuery = useDanmaku(currentVid ? `${currentVid}:${seriesId}` : '');
  const [danmakuOn, setDanmakuOn] = useState(() => readDanmakuEnabled());
  const toggleDanmaku = useCallback(() => {
    setDanmakuOn((on) => {
      writeDanmakuEnabled(!on);
      return !on;
    });
  }, []);
  // 弹幕拉取失败不能静默：画面照常播，但用户该知道弹幕为什么没了
  useEffect(() => {
    if (danmakuQuery.isError) {
      toast.error(tf('player.danmakuFailed', { reason: String(danmakuQuery.error) }));
    }
  }, [danmakuQuery.isError, danmakuQuery.error]);

  /**
   * 兼容兜底。
   *
   * `videoWidth === 0` 而声音正常，是「系统解不了这一集编码」的确凿信号——
   * 容器解析得出时长、样本照样缓冲到位，唯独送进解码器的码流缺参数集。
   * 这时把这一集转成 H.264 换一条路走，产物落缓存，同一集只转一次。
   *
   * 状态一律带 `key`（集标识）并在**读时**过滤，而不是在换集时清空：
   * 清空要在 effect 里同步 setState，会触发级联渲染（eslint 会拦）；
   * 带 key 读时过滤则是天生正确的——上一集的产物自动失效，不需要谁去清它。
   */
  const [compatResult, setCompatResult] = useState<{ key: string; url: string } | null>(null);
  const [compatProgress, setCompatProgress] = useState<{
    key: string;
    percent: number;
    phase: string;
  } | null>(null);
  /** 同一集只兜底一次：失败后允许重试，成功后不再触发 */
  const compatStarted = useRef(false);

  /** 兜底产物地址，只认当前这一集 */
  const compatSrc = compatResult?.key === episodeKey ? compatResult.url : null;
  /** 兜底进度，只认当前这一集 */
  const compat = compatProgress?.key === episodeKey ? compatProgress : null;
  /** 实际喂给 `<video>` 的地址：有兜底产物就用它 */
  const playSrc = compatSrc ?? src;

  useEvent<CompatProgress>(
    EVENTS.compatPlayProgress,
    useCallback((p: CompatProgress) => {
      setCompatProgress({ key: p.key, percent: p.percent, phase: p.phase });
    }, []),
  );

  const startCompat = useCallback(() => {
    if (!seriesId || !vidIndex || compatStarted.current) return;
    compatStarted.current = true;
    setCompatProgress({ key: episodeKey, percent: 0, phase: 'downloading' });
    const ep = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex);
    compatPlay.mutate(
      { seriesId, vidIndex, vid: ep?.vid },
      {
        onSuccess: (r) => {
          setCompatResult({ key: episodeKey, url: r.url });
          setCompatProgress(null);
          toast.success(
            r.cached
              ? t('player.compatCached')
              : tf('player.compatDone', {
                  backend: r.backend,
                  seconds: Math.round(r.elapsedMs / 100),
                }),
          );
        },
        onError: (e) => {
          setCompatProgress(null);
          compatStarted.current = false;
          toast.error(tf('player.compatFailed', { reason: e.message }));
        },
      },
    );
  }, [seriesId, vidIndex, episodeKey, currentSeries, compatPlay]);

  // 解码失败探测：播放在走、画面出不来、且时间确实在推进。
  // 三个条件缺一不可——刚起播那一瞬间 videoWidth 本来就是 0。
  useEffect(() => {
    if (!playSrc || compatSrc || error) return;
    const v = videoRef.current;
    if (!v) return;
    const timer = setInterval(() => {
      if (v.videoWidth === 0 && !v.paused && v.currentTime > 0.3) {
        clearInterval(timer);
        startCompat();
      }
    }, 1200);
    return () => clearInterval(timer);
  }, [playSrc, compatSrc, error, startCompat]);

  // 换集就换一把「已兜底过」的记号：产物与进度都靠 key 自己失效，
  // 这里只需允许新的一集再兜底一次。
  useEffect(() => {
    compatStarted.current = false;
  }, [episodeKey]);

  const stepEpisode = useCallback(
    (delta: number) => {
      if (!seriesId || !vidIndex) return;
      const next = vidIndex + delta;
      if (next < 1) return;
      setTarget(seriesId, next);
    },
    [seriesId, vidIndex, setTarget],
  );

  /**
   * 记录播放位置。
   *
   * `force` 绕过节流：暂停、播完、离开页面这三种时刻之后不会再有下一次
   * timeupdate，被节流挡掉就等于这一段进度永久丢失——「看了 3 秒就切走」
   * 正好落在这 5 秒窗口里，回来又是 0。
   */
  const persist = useCallback(
    (time: number, force = false) => {
      if (!seriesId || !vidIndex) return;
      const now = Date.now();
      if (!force && now - lastSaved.current < SAVE_INTERVAL) return;
      lastSaved.current = now;
      // 时长直接从元素读：媒体状态归自绘控件管，这里不再维护第二份，
      // 免得两处对不上。后端靠它判断「接近片尾就别续播」。
      const video = videoRef.current;
      const total = video && Number.isFinite(video.duration) ? video.duration : 0;
      lastKnown.current = { key: episodeKey, time, duration: total };
      savePosition({ seriesId, vidIndex, currentTime: time, duration: total });
    },
    [seriesId, vidIndex, episodeKey, savePosition],
  );

  // 卸载 / 切集时补写最后一次。
  //
  // 只靠 timeupdate 的定时保存会丢掉最后一小段：用户看完直接点侧边栏
  // 回列表，组件当场卸载，那 5 秒内攒下的位置一次都没落过盘。
  // 这里读的是 lastKnown 而不是 videoRef —— cleanup 跑的时候 video 元素已经被卸载了。
  useEffect(() => {
    if (!seriesId || !vidIndex) return;
    return () => {
      const { key, time, duration } = lastKnown.current;
      // 从头就没播过（加载失败、秒退）不写：否则会给从未看过的集
      // 落一条 0 秒记录，把「继续观看」里凭空多出一张卡。
      // key 对不上说明这份位置属于别的集，写进去就是串集。
      if (key !== episodeKey || time <= 0) return;
      savePosition({ seriesId, vidIndex, currentTime: time, duration });
    };
  }, [seriesId, vidIndex, episodeKey, savePosition]);

  // 起播。依赖里带 definition：切清晰度要重新取流，
  // 而 `<video src>` 换 URL 会重置 currentTime，所以先把当前位置存进
  // pendingSeek —— 否则用户从 10 分钟处切到 720p 会被弹回片头。
  useEffect(() => {
    if (!seriesId || !vidIndex) return;

    const video = videoRef.current;
    if (video && video.currentTime > 0) {
      lastKnown.current = {
        key: episodeKey,
        time: video.currentTime,
        duration: Number.isFinite(video.duration) ? video.duration : lastKnown.current.duration,
      };
    }
    // 切清晰度时续播位置要接着当前播放点，而不是回到「上次看的进度」——
    // 那会把人从 10 分钟处弹回上次退出点，看着像「切清晰度丢了进度」。
    // 只认属于本集的那份：换集后 lastKnown 里是上一集的秒数，拿来续播就串集了。
    const keepPosition = lastKnown.current.key === episodeKey ? lastKnown.current.time : 0;

    play(
      { seriesId, vidIndex, definition },
      {
        onSuccess: (res) => {
          setError(res.error || null);
          setSrc(res.error ? null : res.url);
          setActiveDefinition(res.definition);
          setDefinitions(res.definitions);
          // 续播位置要在 metadata 加载后 seek
          pendingSeek.current = res.error ? 0 : keepPosition > 0 ? keepPosition : res.resumeAt;
        },
        onError: (e) => {
          setError(e.message);
          setSrc(null);
        },
      },
    );
  }, [seriesId, vidIndex, definition, episodeKey, play]);

  const handleLoadedMetadata = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (pendingSeek.current > 0 && pendingSeek.current < video.duration) {
      video.currentTime = pendingSeek.current;
    }
    pendingSeek.current = 0;
    // metadata 就绪后再显式播一次：autoPlay 属性在带声音时会被 WebView 拦下，
    // 只靠属性的表现就是「自动播一下就停住」，必须在这里补一次 play()。
    void video.play().catch(() => undefined);
  }, []);

  // 每次渲染后把倍速/音量/静音贴回当前元素。
  // 依赖数组故意留空：元素被重建的时机（切集、加载、转码）不由本组件的依赖决定，
  // 漏掉任何一次重建，用户设的倍速就会在切集后悄悄回到 1。
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    if (Math.abs(video.playbackRate - playbackRateRef.current) > 0.001) {
      video.playbackRate = playbackRateRef.current;
    }
    if (Math.abs(video.volume - volumeRef.current) > 0.001) {
      video.volume = volumeRef.current;
    }
    if (video.muted !== mutedRef.current) {
      video.muted = mutedRef.current;
    }
  });

  /** 记住用户改的倍速（原生 controls 菜单触发） */
  const handleRateChange = useCallback((e: React.SyntheticEvent<HTMLVideoElement>) => {
    const rate = e.currentTarget.playbackRate;
    if (Number.isFinite(rate) && rate > 0) {
      playbackRateRef.current = rate;
      writePlaybackRate(rate);
    }
  }, []);

  /** 记住用户改的音量与静音（静音也会触发 volumechange，所以两项一起记） */
  const handleVolumeChange = useCallback((e: React.SyntheticEvent<HTMLVideoElement>) => {
    const el = e.currentTarget;
    if (Number.isFinite(el.volume) && el.volume >= 0 && el.volume <= 1) {
      volumeRef.current = el.volume;
      writeVolume(el.volume);
    }
    if (el.muted !== mutedRef.current) {
      mutedRef.current = el.muted;
      writeMuted(el.muted);
    }
  }, []);

  // 键盘快捷键：空格 / ←→ / ↑↓。依赖显式列出，避免每次渲染重绑。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const video = videoRef.current;
      if (!video) return;
      // 输入框里打字时不劫持按键
      const el = e.target as HTMLElement | null;
      if (el && ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName)) return;

      switch (e.key) {
        case ' ':
          e.preventDefault();
          if (video.paused) void video.play();
          else video.pause();
          break;
        case 'ArrowLeft':
          e.preventDefault();
          video.currentTime = Math.max(0, video.currentTime - 5);
          break;
        case 'ArrowRight':
          e.preventDefault();
          video.currentTime = Math.min(video.duration, video.currentTime + 5);
          break;
        case 'ArrowUp':
          e.preventDefault();
          stepEpisode(-1);
          break;
        case 'ArrowDown':
          e.preventDefault();
          stepEpisode(1);
          break;
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [stepEpisode]);

  const handleEnded = () => {
    // 这个函数写在 `!seriesId || !vidIndex` 的提前返回之前，
    // TS 还没把 store 里的 null 收窄掉，所以这里得自己挡一道。
    // 元素本身确实存在（ended 只能由它自己触发），但不写检查就只能写 `!`。
    const video = videoRef.current;
    if (!seriesId || !vidIndex || !video) return;
    persist(video.currentTime, true);
    // 看完自动删：先清掉刚看完这集的本地文件，再决定连播下一集。
    // 没下载过的集本来就没有文件，后端返回 false，不提示。
    if (autoDelete) {
      deleteEpisode.mutate(
        { seriesId, vidIndex },
        {
          onSuccess: (deleted) => {
            if (deleted) toast.success(tf('player.autoDeleteDone', { index: vidIndex }));
          },
        },
      );
    }
    // 下载面板开着就连播：切集会让 PlayerView 带着 key 整体重挂载，
    // 面板连同勾选一起消失，用户刚选完的集就没了。
    if (autoNext && !downloading) stepEpisode(1);
  };

  const handleVideoError = () => {
    if (error) return;
    // 带上浏览器给的 MediaError 编号：解码错(3)和网络错(2)的修法完全不同
    const me = videoRef.current?.error;
    const detail = me ? `（MediaError ${me.code}${me.message ? `: ${me.message}` : ''}）` : '';
    setError(`${t('error.media')}${detail}`);
  };

  if (!seriesId || !vidIndex) {
    return (
      <div className="grid h-full place-items-center p-6">
        <p className="text-muted-foreground text-sm">{t('player.noSeries')}</p>
      </div>
    );
  }

  return (
    <div className="flex h-full gap-4 p-4">
      <div className="flex min-w-0 flex-1 flex-col gap-3">
        <div ref={stageRef} className="relative min-h-0 flex-1 overflow-hidden rounded-lg bg-black">
          {playSrc ? (
            <>
              {/* 自绘控件，不要原生 controls：它既不跟主题，也放不下下载/清晰度这类业务动作。
                  key 绑 src：切清晰度时后端给出的流地址变了（地址里带档位），
                  换元素才能保证 <video> 真的重新加载——只改 src 属性在
                  WebView2 上不一定会触发重载，表现就是「点了 720p 画面不变」。 */}
              <video
                /* key 绑播放地址：切清晰度、以及兜底转成 H.264 之后地址都变了，
                   换元素才能保证 <video> 真的重新加载——只改 src 属性在
                   WebView2 上不一定会触发重载，表现就是「点了 720p 画面不变」。 */
                key={playSrc}
                ref={videoRef}
                src={playSrc}
                className="size-full"
                autoPlay
                onLoadedMetadata={handleLoadedMetadata}
                onTimeUpdate={(e) => persist(e.currentTarget.currentTime)}
                onPause={(e) => persist(e.currentTarget.currentTime, true)}
                onEnded={handleEnded}
                onRateChange={handleRateChange}
                onVolumeChange={handleVolumeChange}
                onError={handleVideoError}
              />
              <DanmakuLayer
                videoRef={videoRef}
                items={danmakuQuery.data ?? []}
                enabled={danmakuOn}
              />
              <PlayerControls
                videoRef={videoRef}
                stageRef={stageRef}
                seriesId={seriesId}
                episodes={currentSeries?.episodes ?? []}
                currentIndex={vidIndex}
                downloading={downloading}
                onDownloadingChange={setDownloading}
                onStepEpisode={stepEpisode}
                definition={activeDefinition}
                definitions={definitions}
                onDefinitionChange={setDefinition}
                src={playSrc}
                danmakuOn={danmakuOn}
                onToggleDanmaku={toggleDanmaku}
              />

              {/* 兜底转码浮层。转一集要几十秒，没有它用户只能盯着黑屏，
                  不知道是卡住了还是在慢慢转。 */}
              {compat && (
                <div className="absolute inset-0 grid place-items-center bg-black/85 p-6 text-center text-sm text-neutral-200">
                  <div className="flex w-full max-w-sm flex-col items-center gap-3">
                    <p>
                      {compat.phase === 'downloading'
                        ? t('player.compatFetching')
                        : tf('player.compatTranscoding', { percent: Math.floor(compat.percent) })}
                    </p>
                    <Progress
                      value={compat.phase === 'downloading' ? 0 : compat.percent}
                      className="h-1.5"
                    />
                    <span className="text-xs text-neutral-400">{t('player.compatHint')}</span>
                  </div>
                </div>
              )}
            </>
          ) : (
            <div className="text-muted-foreground grid size-full place-items-center text-sm">
              {/* 缓冲时给的是「在动到哪了」，不是一个没头没尾的转圈 */}
              {error ??
                (buffering && buffering.phase !== 'ready'
                  ? tf('player.buffering', {
                      percent: buffering.total > 0 ? Math.floor(buffering.percent) : 0,
                      size:
                        buffering.total > 0
                          ? `${formatBytes(buffering.received)} / ${formatBytes(buffering.total)}`
                          : formatBytes(buffering.received),
                    })
                  : t('common.loading'))}
            </div>
          )}
        </div>

        {error && (
          <Card className="py-2 text-sm">
            <span className="text-destructive">{error}</span>
          </Card>
        )}

        {/* 原生 controls 里已经有时间与进度条，这里不再重复一份 */}
        <div className="ml-auto flex items-center gap-4">
          <div className="flex items-center gap-2">
            <Switch
              id="auto-next"
              checked={autoNext}
              disabled={settingsPending}
              onCheckedChange={(v) => patchSettings({ autoNextEpisode: v })}
            />
            <Label htmlFor="auto-next" className="text-sm">
              {t('player.autoNext')}
            </Label>
          </div>
          <div className="flex items-center gap-2">
            <Switch
              id="auto-delete"
              checked={autoDelete}
              disabled={settingsPending}
              onCheckedChange={(v) => patchSettings({ autoDeleteAfterPlay: v })}
            />
            <Label htmlFor="auto-delete" className="text-sm">
              {t('player.autoDelete')}
            </Label>
          </div>
        </div>

        <p className="text-muted-foreground text-xs">{t('player.keyboardHint')}</p>
      </div>

      <SeriesPanel
        seriesId={seriesId}
        currentIndex={vidIndex}
        onSelect={(index) => setTarget(seriesId, index)}
      />
    </div>
  );
}

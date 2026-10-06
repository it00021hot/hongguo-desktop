import { useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate } from '@tanstack/react-router';
import { Loader2, MonitorPlay } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import { Progress } from '@/components/ui/progress';
import { Label } from '@/components/ui/label';
import { DanmakuLayer } from './danmaku-layer';
import { PlayerControls } from './player-controls';
import { InteractionRail } from './interaction-rail';
import { CommentPanel } from './comment-panel';
import {
  usePlay,
  useSavePosition,
  useSaveSettings,
  useCompatPlayback,
  useDanmaku,
  useSeriesEpisodes,
  useSeriesExtras,
  useSettings,
  useStorageActions,
  useWatchHistory,
} from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import {
  readDanmakuDisplay,
  readDanmakuEnabled,
  readLastTarget,
  readMuted,
  readPlaybackRate,
  readVolume,
  writeDanmakuDisplay,
  writeDanmakuEnabled,
  writeMuted,
  writePlaybackRate,
  writeVolume,
  type DanmakuDisplaySettings,
} from '@/lib/playback-prefs';
import { t, tf } from '@/i18n';
import { cn } from '@/lib/utils';
import { useEvent } from '@/lib/ipc/events';
import { watchHistory } from '@/lib/ipc/commands';
import { EVENTS } from '@/lib/ipc/types';
import { formatBytes } from '@/lib/format';
import type { CompatProgress, OnlineProgress, Settings, VideoDefinition } from '@/lib/schema';

/** 进度保存间隔（毫秒）。太频繁会写爆磁盘，太稀疏丢进度。 */
const SAVE_INTERVAL = 5_000;

export function PlayerPage() {
  const navigate = useNavigate();
  const seriesId = usePlayerStore((s) => s.seriesId);
  const vidIndex = usePlayerStore((s) => s.vidIndex);
  const setTarget = usePlayerStore((s) => s.setTarget);
  // 封面占位图：历史页带封面进来，取流间隙不至于黑屏（信息流入口由 HomePage 自带）
  const { data: history } = useWatchHistory();
  const coverUrl = history?.items.find((i) => i.seriesId === seriesId)?.cover;
  // 弹幕设置面板的开合放在这一层：切集时 PlayerView 整体重挂载，
  // 面板状态在这里才不会一集一开就被吃掉

  // 音量竖条浮层同样在这层持有：切集重挂载不会把正开着的浮层收走


  // 刷新/重启后内存 store 是空的：把上次播放目标读回来，
  // 播放器直接续播（进度由本地播放档案的 resumeAt 接上）。
  // 连上次播放目标都没有（首次启动/清过记录）：直接进推荐沉浸流——
  // 打开就能播（第三方同款），而不是摆一个还要自己去找剧的空态；
  // 推荐流里 ↑↓/滚轮 = 切剧，从详情/历史**选定**剧进来才是 ↑↓ = 切集。
  useEffect(() => {
    if (seriesId) return;
    const last = readLastTarget();
    if (last) {
      setTarget(last.seriesId, last.vidIndex);
      return;
    }
    void navigate({ to: '/' });
  }, [seriesId, setTarget, navigate]);

  // 兜底空态：正常只在重定向生效前闪一帧
  if (!seriesId || !vidIndex) {
    return <PlayerEmptyState />;
  }

  // key 随剧集变化 → 切集时组件整体重建，播放/转码状态自然清零，
  // 不必在 effect 里同步 setState（那会触发级联渲染）。
  return (
    <PlayerView
      key={`${seriesId}:${vidIndex}`}
      coverUrl={coverUrl}
    />
  );
}

/** 未在播放时的占位：去推荐/历史挑一部剧即可开始。 */
function PlayerEmptyState() {
  const navigate = useNavigate();
  return (
    <div className="text-muted-foreground grid h-full place-items-center p-6">
      <div className="flex flex-col items-center gap-3 text-center">
        <MonitorPlay className="size-10 opacity-40" aria-hidden />
        <p className="text-sm">{t('player.empty')}</p>
        <p className="text-xs opacity-70">{t('player.emptyHint')}</p>
        <div className="mt-2 flex gap-2">
          <Button size="sm" variant="outline" onClick={() => void navigate({ to: '/history' })}>
            {t('nav.history.title')}
          </Button>
          <Button size="sm" variant="outline" onClick={() => void navigate({ to: '/' })}>
            {t('nav.home.title')}
          </Button>
        </div>
      </div>
    </div>
  );
}

/** 播放器悬浮层（信息/互动栏）静止多久后淡出。与控制栏的 3 秒同款。 */
const CHROME_HIDE_MS = 3_000;

export function PlayerView({
  onWheelStep,
  coverUrl,
  topChrome,
}: {
  /**
   * 滚轮/↑↓ 的语义由宿主页给：首页（沉浸流）是切**上一部/下一部剧**，
   * 不给则回落为切上一集/下一集（从历史/收藏等页进入时）。
   */
  onWheelStep?: (dir: 1 | -1) => void;
  /**
   * 封面占位图（信息流条目/历史记录都有）：切剧/起播的取流间隙垫在画面上，
   * 首帧真正出画（playing）后交叉淡出——第三方「滚动秒切」的观感一半来自这里，
   * 黑屏等待变成封面常驻，感知延迟只剩取流本身。
   */
  coverUrl?: string;
  /**
   * 压在画面顶部居中的悬浮内容（沉浸流的分类 tab 栏）。
   * 跟随悬浮层一起淡出，data-wheel-block 标记：点击/滚动不冒泡成
   * 「点画面暂停」「滚轮切剧」。
   */
  topChrome?: React.ReactNode;
}) {
  const navigate = useNavigate();
  const seriesPanelOpen = usePlayerStore((s) => s.seriesPanelOpen);
  const commentPanelOpen = usePlayerStore((s) => s.commentPanelOpen);
  const danmakuPanelOpen = usePlayerStore((s) => s.danmakuPanelOpen);
  const volumeOpen = usePlayerStore((s) => s.volumeOpen);
  const setCommentPanelOpen = usePlayerStore((s) => s.setCommentPanelOpen);
  const videoRef = useRef<HTMLVideoElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  /**
   * 最近一次切换的方向（1=下一个/向下滚，-1=上一个）。滚轮/↑↓/连播在
   * 触发切换时记下，流地址变化的那次渲染据此决定新内容从下边还是上边
   * 滑入——抖音式「内容跟手」的过渡感全靠这一个方向的符号。
   * 事件写 state 而不是 ref：切换动作到流交换之间必然隔至少一次渲染，
   * 渲染期读到的一定是本次切换的方向（渲染期读 ref 会被 react-hooks 拦）。
   */
  const [slideDir, setSlideDir] = useState<1 | -1>(1);
  const lastSaved = useRef(0);
  /** 云端进度上报计数（配合 persist 的 5s 节流折算 ~1 分钟一次） */
  const cloudCounter = useRef(0);
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
  /**
   * 现在这条流属于哪一集（episodeKey）。信息流切剧时 play 请求在途、
   * 旧流还在播（timeupdate 一直在来），不带这道闸会把旧画面的秒数
   * 记到新剧头上——续播位置凭空串剧。
   */
  const srcKeyRef = useRef('');

  // 切集 / 加载中都会让 <video> 被卸载重建，新元素的倍速音量静音
  // 一律回到默认值，所以「用户设的值」要存在这里，每次渲染后再贴回元素。
  const playbackRateRef = useRef(readPlaybackRate());
  const volumeRef = useRef(readVolume());
  const mutedRef = useRef(readMuted());

  const seriesId = usePlayerStore((s) => s.seriesId);
  const vidIndex = usePlayerStore((s) => s.vidIndex);
  const setTarget = usePlayerStore((s) => s.setTarget);
  // 选中剧连播：锁定后滚轮/↑↓ 切集而不是跟随宿主页换剧
  const bingeSeriesId = usePlayerStore((s) => s.bingeSeriesId);
  const setBinge = usePlayerStore((s) => s.setBinge);
  const inBinge = bingeSeriesId != null && bingeSeriesId === seriesId;
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
  /**
   * 现在这条流属于哪一集（srcKeyRef 的 state 镜像，同点写入）。
   * 渲染期据此判定「切换在途」：目标已变、新流未到——旧画面还在播，
   * 但新剧的封面+加载胶囊要立即接管，否则用户看到的就是「切了没反应」。
   */
  const [srcKey, setSrcKey] = useState('');
  /**
   * 在线流断供的自动重试（带集指纹，读时校验，换集自动失效）。
   *
   * 渐进流的数据面断了（切剧竞态、网络抖动）会让 <video> 报 MediaError；
   * 重新 prepare 一次就能拿到新流，续播位置由 lastKnown 接上。每集只自动
   * 兜一次——再失败多半不是抖动，亮出重试按钮交给用户。tick 进起播
   * effect 的依赖驱动重新取流；配对的 setSrc(null) 把 <video> 卸载，
   * 重挂载才会真的重新加载（URL 不变时只换 src 属性在 WebView2 上未必
   * 触发重载）。
   */
  const [retry, setRetry] = useState<{ key: string; tick: number }>({ key: '', tick: 0 });
  const retryTick = retry.key === episodeKey ? retry.tick : 0;
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

  /** 重新取流接续播放（自动重试与手动重试按钮共用）。 */
  const retryOnline = useCallback(() => {
    setError(null);
    setSrc(null);
    setRetry((r) => ({ key: episodeKey, tick: (r.key === episodeKey ? r.tick : 0) + 1 }));
  }, [episodeKey]);

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
  // 简介只在沉浸流信息叠加里用（播放页右侧面板自己拉）
  const { data: extras } = useSeriesExtras(seriesId ?? '');

  // 弹幕：vid 来自剧集档案的分集表，换集自动换一份缓存
  const currentVid = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex)?.vid ?? '';
  // 当前集的公开计数（detail 接口下发；右栏 ♥/💬 数字）
  const currentEpisode = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex);
  const danmakuQuery = useDanmaku(currentVid ? `${currentVid}:${seriesId}` : '');
  const [danmakuOn, setDanmakuOn] = useState(() => readDanmakuEnabled());
  const [danmakuDisplay, setDanmakuDisplay] = useState<DanmakuDisplaySettings>(
    () => readDanmakuDisplay(),
  );
  const updateDanmakuDisplay = useCallback((patch: Partial<DanmakuDisplaySettings>) => {
    setDanmakuDisplay((prev) => {
      const next = { ...prev, ...patch };
      writeDanmakuDisplay(next);
      return next;
    });
  }, []);
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
  const baseSrc = compatSrc ?? src;
  /**
   * 重试要换 URL：WebView2 可能缓存了失败瞬间的坏响应（自定义协议历史上
   * 没带 no-store），同一个地址重试永远拿坏数据。后端处理器只解析 path，
   * query 是纯缓存钉（后端已补 no-store，这层双保险兜老进程/旧缓存）。
   */
  const playSrc =
    baseSrc && baseSrc.includes('hongguo-stream') && retryTick > 0
      ? `${baseSrc}?r=${retryTick}`
      : baseSrc;
  /** 当前流的指纹：live/stalled 状态读时校验它，换流（切剧/切集/换清晰度）即失效 */
  const streamKey = playSrc ?? '';
  /** 切换在途：目标已是新的一集，元素里还是上一条流（play 请求未返回） */
  const switching = srcKey !== episodeKey;

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
          srcKeyRef.current = episodeKey;
          setSrcKey(episodeKey);
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
      // 连播模式下滚到尾部要有交代，静默不动像坏了
      const total = currentSeries?.episodes.length ?? 0;
      if (total > 0 && next > total) {
        toast.info(tf('player.lastEpisode', { index: total }));
        return;
      }
      setSlideDir(delta > 0 ? 1 : -1);
      setTarget(seriesId, next);
    },
    [seriesId, vidIndex, currentSeries, setTarget],
  );

  // ---- 沉浸流悬浮层：鼠标在画面内就常显，移出画面即隐藏 ----
  // 第三方同款：光标停在画面上（哪怕一动不动）控制栏不该消失——
  // 「3 秒不动就藏」逼着用户不停晃鼠标，反而更打扰。
  // 暂停状态跟 <video> 走（onPlay/onPause），悬浮层的「常显」语义在这里统一裁决
  const [paused, setPaused] = useState(true);
  const [stageHovered, setStageHovered] = useState(false);
  const [chromeVisible, setChromeVisible] = useState(true);
  /** 隐藏倒计时的代际号：每次唤醒递增，倒计时 effect 随之重启 */
  const [chromeTick, setChromeTick] = useState(0);
  const wakeChrome = useCallback(() => {
    setChromeVisible(true);
    setChromeTick((n) => n + 1);
  }, []);
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    stage.addEventListener('mousemove', wakeChrome);
    const onEnter = () => {
      setStageHovered(true);
      setChromeVisible(true);
    };
    const onLeave = () => {
      setStageHovered(false);
      setChromeVisible(false);
    };
    stage.addEventListener('mouseenter', onEnter);
    stage.addEventListener('mouseleave', onLeave);
    return () => {
      stage.removeEventListener('mousemove', wakeChrome);
      stage.removeEventListener('mouseenter', onEnter);
      stage.removeEventListener('mouseleave', onLeave);
    };
  }, [wakeChrome]);
  // 隐藏倒计时：只管「光标不在画面上」的场景（键盘快捷键唤醒后没人动
  // 鼠标，亮 3 秒再收）。光标在画面内（stageHovered）或暂停时常显不倒计时。
  useEffect(() => {
    if (paused || stageHovered) return;
    const timer = setTimeout(() => setChromeVisible(false), CHROME_HIDE_MS);
    return () => clearTimeout(timer);
  }, [paused, stageHovered, chromeTick]);

  // 悬浮层整体可见性：任一面板（选集/评论/弹幕设置/音量条）打开或暂停时常显，
  // 其余由上面的倒计时裁决。简介/互动栏/控制栏/顶部杂物全部吃这一个值，
  // 不再各养一套定时器——控制栏弹出时简介同步抬升也是靠它。
  const chromeShown =
    paused ||
    chromeVisible ||
    seriesPanelOpen ||
    commentPanelOpen ||
    danmakuPanelOpen ||
    volumeOpen;

  /** 简介展开态：切剧重挂载自然收回。 */
  const [introExpanded, setIntroExpanded] = useState(false);
  /**
   * 当前流是否已出画 / 正在卡顿。两条状态都带流指纹（streamKey）：
   * 信息流的 PlayerView 是复用的（不随切剧重建），不带指纹的话上一部剧
   * 留下的「已出画」会原样漏给下一部——切剧黑屏期间加载胶囊和封面占位
   * 全被跳过，正是「切剧=纯黑屏、一点反应都没有」的来源。
   * 读时校验指纹：换流即失效，不用在 effect 里追着清 state。
   */
  const [live, setLive] = useState<{ key: string; on: boolean }>({ key: '', on: false });
  const [stalled, setStalled] = useState<{ key: string; on: boolean }>({ key: '', on: false });
  const videoLive = live.key === streamKey && live.on;
  /** waiting 后时间轴恢复推进（timeupdate）或重新出画即视为不卡 */
  const videoStalled = stalled.key === streamKey && stalled.on;

  // ---- 滚轮切换（hgplayer 同款）：沉浸流=上一部/下一部剧，播放页=切集 ----
  // 选集浮层/下载面板打开时不抢滚动；冷却 400ms 防一次惯性滚动连跳。
  const wheelLock = useRef(0);
  const onStageWheel = useCallback(
    (e: React.WheelEvent) => {
      // 浮层（评论面板/选集/弹幕设置/倍速清晰度菜单）里的滚动是它自己在滚，
      // 不冒泡成「切集」。控制栏与面板用 data-wheel-block 标记；Radix 菜单
      // portal 到 body，真实 DOM 里不是舞台子孙，必须各自带标记才拦得住。
      if ((e.target as HTMLElement | null)?.closest?.('[data-wheel-block]')) return;
      // 滚轮也是「用户在场」：切剧/切集时唤醒悬浮层，让新一部的信息亮 3 秒
      wakeChrome();
      if (seriesPanelOpen || downloading) return;
      const now = Date.now();
      if (now - wheelLock.current < 400 || Math.abs(e.deltaY) < 15) return;
      wheelLock.current = now;
      const dir: 1 | -1 = e.deltaY > 0 ? 1 : -1;
      setSlideDir(dir);
      // 连播锁定时滚轮语义变为切集，不跟随宿主页换剧
      if (inBinge) stepEpisode(dir);
      else if (onWheelStep) onWheelStep(dir);
      else stepEpisode(dir);
    },
    [seriesPanelOpen, downloading, onWheelStep, stepEpisode, wakeChrome, inBinge],
  );

  // ---- 点击画面：唤醒悬浮层 + 抖音式播放/暂停 ----
  // 唤醒原来只挂在 mousemove/滚轮上，鼠标停着直接点一下，控件永远不出来。
  // 控件/面板/互动栏（data-wheel-block 标记区）里的点击是它们自己的事，
  // 不冒泡成「点画面暂停」。
  const onStageClick = useCallback(
    (e: React.MouseEvent) => {
      if ((e.target as HTMLElement | null)?.closest?.('[data-wheel-block]')) return;
      wakeChrome();
      const video = videoRef.current;
      if (!video) return;
      if (video.paused) void video.play().catch(() => undefined);
      else video.pause();
    },
    [wakeChrome],
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
      // 流不是这一集的（信息流切剧、play 在途旧流还在播）：秒数不能串到新剧头上
      if (srcKeyRef.current !== episodeKey) return;
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
      // 云端进度上报：登录后每 ~1 分钟一次（SAVE_INTERVAL 5s × 12），
      // 与本地落盘同源同节流，fire-and-forget（后端匿名/失败都静默）。
      cloudCounter.current += 1;
      if (force || cloudCounter.current % 12 === 0) {
        if (currentVid) {
          void watchHistory
            .reportProgress(seriesId, currentVid, vidIndex, Math.round(time * 1000))
            .catch(() => {});
        }
      }
    },
    [seriesId, vidIndex, episodeKey, savePosition, currentVid],
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
      // 切集/退出的最后一次位置也推一份云端（匿名/失败后端静默）
      const vid = currentSeries?.episodes.find((e) => e.vidIndex === vidIndex)?.vid ?? '';
      if (vid) {
        void watchHistory
          .reportProgress(seriesId, vid, vidIndex, Math.round(time * 1000))
          .catch(() => {});
      }
    };
  }, [seriesId, vidIndex, episodeKey, savePosition, currentSeries]);

  // 起播。依赖里带 definition：切清晰度要重新取流，
  // 而 `<video src>` 换 URL 会重置 currentTime，所以先把当前位置存进
  // pendingSeek —— 否则用户从 10 分钟处切到 720p 会被弹回片头。
  useEffect(() => {
    if (!seriesId || !vidIndex) return;

    const video = videoRef.current;
    // 只在元素里还是这一集的流时才续点（切清晰度场景）；信息流切剧时
    // 元素里还是上一部剧的画面，读它的 currentTime 就是串剧
    if (video && video.currentTime > 0 && srcKeyRef.current === episodeKey) {
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
          srcKeyRef.current = episodeKey;
          setSrcKey(episodeKey);
          setError(res.error || null);
          setSrc(res.error ? null : res.url);
          setActiveDefinition(res.definition);
          setDefinitions(res.definitions);
          // 续播位置要在 metadata 加载后 seek
          pendingSeek.current = res.error ? 0 : keepPosition > 0 ? keepPosition : res.resumeAt;
        },
        onError: (e) => {
          srcKeyRef.current = '';
          setSrcKey('');
          setError(e.message);
          setSrc(null);
        },
      },
    );
  }, [seriesId, vidIndex, definition, episodeKey, play, retryTick]);

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
          wakeChrome();
          video.currentTime = Math.max(0, video.currentTime - 5);
          break;
        case 'ArrowRight':
          e.preventDefault();
          wakeChrome();
          video.currentTime = Math.min(video.duration, video.currentTime + 5);
          break;
        case 'ArrowUp':
        case 'ArrowDown': {
          e.preventDefault();
          wakeChrome();
          // ↑↓ 的语义与滚轮同源：沉浸流（未选定剧）= 切上一部/下一部剧，
          // 从详情/历史等**选定**剧进来 = 切上一集/下一集
          const dir: 1 | -1 = e.key === 'ArrowDown' ? 1 : -1;
          setSlideDir(dir);
          if (inBinge) stepEpisode(dir);
          else if (onWheelStep) onWheelStep(dir);
          else stepEpisode(dir);
          break;
        }
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [stepEpisode, wakeChrome, onWheelStep, inBinge]);

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
    // 在线渐进流断供可自愈（重新 prepare 换新流接着播，位置不丢）：
    // 每集只自动兜一次，兜底转码进行中不抢跑；本地文件/转码产物坏了
    // 重试也是白搭，直接报错。在线的失败把 <video> 撤下来，换成封面 +
    // 明确错误 + 重试按钮，不再是一根小小的报错条挂在冻结画面上。
    const online = playSrc?.includes('hongguo-stream') ?? false;
    if (online && !compatSrc && !compat && retryTick < 1) {
      retryOnline();
      return;
    }
    setError(`${online ? t('player.onlineInterrupted') : t('error.media')}${detail}`);
    if (online) setSrc(null);
  };

  if (!seriesId || !vidIndex) {
    return (
      <div className="grid h-full place-items-center p-6">
        <p className="text-muted-foreground text-sm">{t('player.noSeries')}</p>
      </div>
    );
  }

  // 封面占位：盖在 <video> 之上（z-[5] 压过视频、让位弹幕 z-10），首帧出画
  // （playing）后交叉淡出。取流/解码的间隙里用户看到的是这部剧的封面而不是
  // 黑屏——「滚动切剧卡顿」的观感大头在这。取流中（!playSrc）同样垫着。
  const coverBackdrop = coverUrl ? (
    <CoverBackdrop key={coverUrl} src={coverUrl} hidden={videoLive && !switching} />
  ) : null;

  return (
    // 全模式一个视觉：视频区全出血贴边（无 padding/圆角/留白）。
    // 从历史/收藏等页进入与首页沉浸流是**同一个播放器**，选集走控制栏的
    // 「选集」弹层（immersive），不再维护第二套侧栏布局。
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-w-0 flex-1 flex-col">
        <div
          ref={stageRef}
          onWheel={onStageWheel}
          onClick={onStageClick}
          className="relative min-h-0 flex-1 overflow-hidden bg-black"
        >
          {/* 抖音式切换过渡：key 绑「实际供数的流」（取流中旧流继续播，
              动画精确落在新内容出画的那一帧），内容整体按方向滑入
              （下一个从下、上一个从上）+淡入——配合封面占位读作「翻页」，
              而不是硬切。方向在事件里先写进 state 再换目标：切换动作到
              流地址交换之间必然隔至少一次渲染，这次渲染读到的符号就是
              本次切换的方向。 */}
          <div
            key={playSrc ?? episodeKey}
            className={cn(
              'absolute inset-0 ease-out animate-in fade-in duration-300',
              slideDir === -1 ? 'slide-in-from-top-10' : 'slide-in-from-bottom-10',
            )}
          >
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
                /* 绝对定位铺满舞台：WKWebView 在流式布局里会把 100% 高度的
                   <video> 按固有纵横比撑大（1100 宽 → 1956 高），舞台裁切后
                   表现为「画面放大、控件全被顶出窗口」（mac 长期已知 bug）。
                   绝对定位把盒子钉死在舞台内，object-contain 只管留黑边。 */
                className="absolute inset-0 size-full object-contain"
                autoPlay
                onLoadedMetadata={handleLoadedMetadata}
                onPlay={() => {
                  // 恢复播放（含起播）也算「用户在场」：重新亮 3 秒再淡出，
                  // 不然暂停期间常显的界面会在恢复的一瞬全部消失
                  setPaused(false);
                  wakeChrome();
                }}
                onPlaying={() => {
                  // 真正出画/恢复出画：封面占位退场、卡顿态收掉
                  setLive({ key: streamKey, on: true });
                  setStalled({ key: streamKey, on: false });
                }}
                onPause={(e) => {
                  setPaused(true);
                  persist(e.currentTarget.currentTime, true);
                }}
                onLoadedData={() => setLive({ key: streamKey, on: true })}
                // waiting：缓冲/seek 供不上数据，画面停住转黑——必须给出「在动」的信号，
                // 否则网络一抖就是一帧黑屏挂在那里，观感等于卡死
                onWaiting={() => setStalled({ key: streamKey, on: true })}
                onCanPlay={() => setStalled({ key: streamKey, on: false })}
                onTimeUpdate={(e) => {
                  // playing 事件在个别 WebView 起播路径上不触发：封面退场
                  // 不能只靠它一路信号，时间轴真的走起来了也算出画
                  if (e.currentTarget.currentTime > 0.1) setLive({ key: streamKey, on: true });
                  // 时间轴在推进本身就是「没卡住」的证据，waiting 的卡顿态在这里收掉
                  setStalled({ key: streamKey, on: false });
                  persist(e.currentTarget.currentTime);
                }}
                onEnded={handleEnded}
                onRateChange={handleRateChange}
                onVolumeChange={handleVolumeChange}
                onError={handleVideoError}
              />
              {coverBackdrop}
              <DanmakuLayer
                videoRef={videoRef}
                items={danmakuQuery.data ?? []}
                enabled={danmakuOn}
                display={danmakuDisplay}
              />
              {/* 沉浸流信息叠加（hgplayer 同款）：@剧名/集数/简介压在画面左下。
                  控制栏弹出时整体抬到控制栏上沿之上（bottom-28），隐藏时落回
                  bottom-14——两者transition 联动，不再互相遮挡。 */}
              <div
                className={cn(
                  'absolute left-3 z-10 max-w-[62%] transition-all duration-300',
                  chromeShown ? 'bottom-28 opacity-100' : 'bottom-14 opacity-0 pointer-events-none',
                )}
              >
                {/* 剧名 → 详情页。第三方同款交互：点标题离开播放器看档案/选集。
                    stopPropagation：点标题不能同时触发「点画面暂停」。 */}
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    void navigate({ to: '/detail', search: { seriesId } });
                  }}
                  className="cursor-pointer text-sm font-semibold text-white drop-shadow-md hover:underline"
                  title={currentSeries?.title}
                >
                  @{currentSeries?.title ?? ''}
                </button>
                <p className="mt-0.5 text-xs text-white/85 drop-shadow-md">
                  {tf('player.epShort', { index: vidIndex })}
                  {currentSeries && currentSeries.episodeCount > 0 && (
                    <span className="text-white/70">
                      {' · '}
                      {tf('player.totalEpisodes', { count: currentSeries.episodeCount })}
                    </span>
                  )}
                </p>
                {extras?.intro && (
                  <div className="mt-1 flex items-end gap-2">
                    <p
                      role="button"
                      tabIndex={0}
                      onClick={(e) => {
                        e.stopPropagation();
                        setIntroExpanded((v) => !v);
                      }}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' || e.key === ' ') setIntroExpanded((v) => !v);
                      }}
                      className={cn(
                        'cursor-pointer text-xs leading-relaxed text-white/70 drop-shadow-md',
                        !introExpanded && 'line-clamp-2',
                      )}
                    >
                      {extras.intro}
                    </p>
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        setIntroExpanded((v) => !v);
                      }}
                      className="shrink-0 cursor-pointer pb-0.5 text-xs text-white/60 drop-shadow-md hover:text-white"
                    >
                      {introExpanded ? t('player.introCollapse') : t('player.introExpand')}
                    </button>
                  </div>
                )}
              </div>
              {/* 快捷键提示只在暂停时露一面向中部提示——常驻顶栏会把分类 tab
                  挡死（顶部让位给 tab），平时不打扰。 */}
              {paused && !error && (
                <div className="pointer-events-none absolute inset-x-0 top-[38%] z-10 flex justify-center">
                  <span className="rounded-full bg-black/55 px-4 py-1.5 text-xs text-white/75 backdrop-blur-sm">
                    {t('player.keyboardHint')}
                  </span>
                </div>
              )}
              {/* 顶部居中悬浮插槽（沉浸流分类 tab 栏）：跟随悬浮层淡出。
                  居中用 flex 而不是 -translate-x-1/2——半像素变换会让文字
                  整体落在亚像素位置，WebView 里渲染出来就是糊的。
                  外层 pointer-events-none 让两侧空带照常点画面/滚轮；
                  只有胶囊本体（data-wheel-block）拦截交互。 */}
              {topChrome && (
                <div
                  className={cn(
                    'pointer-events-none absolute inset-x-0 top-3 z-20 flex justify-center',
                    'transition-opacity duration-300',
                    chromeShown ? 'opacity-100' : 'opacity-0',
                  )}
                >
                  <div data-wheel-block className="pointer-events-auto">
                    {topChrome}
                  </div>
                </div>
              )}
              {/* 沉浸流评论区：右侧滑出（💬 触发） */}
              {commentPanelOpen && (
                <CommentPanel
                  seriesId={seriesId}
                  vid={currentVid ? `${currentVid}:${seriesId}` : ''}
                  onClose={() => setCommentPanelOpen(false)}
                />
              )}
              {/* 互动栏（点赞/评论/收藏/预约/分享，抖音系右缘形态）：跟随悬浮层淡出；
                  评论区面板打开时让位隐藏（面板就盖在右缘，Esc 或 💬 再开）。
                  vid 是「vid:seriesId」组合形态（与弹幕缓存 key 同构），组件内部自行拆用。 */}
              <InteractionRail
                seriesId={seriesId}
                vid={currentVid ? `${currentVid}:${seriesId}` : ''}
                visible={chromeShown && !commentPanelOpen}
                title={currentSeries?.title}
                // 公开计数来自剧集档案（detail 接口逐集下发），匿名可见
                commentCount={currentEpisode?.commentCount}
                diggCount={currentEpisode?.diggCount}
                followCount={currentSeries?.followedCnt}
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
                danmakuDisplay={danmakuDisplay}
                onDanmakuDisplayChange={updateDanmakuDisplay}
                immersive
                visible={chromeShown}
                // 信息流里主动点了某一级 = 要追这部：进入选中剧连播（/player，
                // 滚轮/↑↓ 自动变为切集），而不是留在推荐流继续换剧
                pickerHint={onWheelStep ? t('player.pickerBingeHint') : undefined}
                onPickEpisode={(idx) => {
                  setTarget(seriesId, idx);
                  // 信息流里主动选了某集 = 要追这部：原地锁定本剧连播
                  // （滚轮/↑↓ 切集 + 左上角亮出连播指示），画面无缝续播
                  if (onWheelStep) setBinge(seriesId);
                }}
              />

              {/* 兜底转码浮层。转一集要几十秒，没有它用户只能盯着黑屏，
                  不知道是卡住了还是在慢慢转。z-30：压过封面占位。 */}
              {compat && (
                <div className="absolute inset-0 z-30 grid place-items-center bg-black/85 p-6 text-center text-sm text-neutral-200">
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

              {/* 流地址已就绪、首帧还没出画（解码/缓冲）的这一段也要有指示：
                  只有封面垫着没有任何「正在动」的信号，观感就是卡死了。
                  播放中途 waiting（网络抖动/seek 供数）同样收进来。 */}
              {(switching || !videoLive || (videoStalled && !paused)) && !compat && (
                <div className="pointer-events-none absolute inset-0 z-20 grid place-items-center p-6">
                  <span className="flex items-center gap-2 rounded-full bg-black/60 px-4 py-1.5 text-xs text-white/85 backdrop-blur-sm">
                    <Loader2 className="size-3.5 animate-spin" aria-hidden />
                    {buffering && buffering.phase !== 'ready'
                      ? tf('player.buffering', {
                          percent: buffering.total > 0 ? Math.floor(buffering.percent) : 0,
                          size:
                            buffering.total > 0
                              ? `${formatBytes(buffering.received)} / ${formatBytes(buffering.total)}`
                              : formatBytes(buffering.received),
                        })
                      : t('common.loading')}
                  </span>
                </div>
              )}
            </>
          ) : (
            <>
              {coverBackdrop}
              <div className="absolute inset-0 z-10 grid place-items-center p-6">
                {/* 缓冲时给的是「在动到哪了」，不是一个没头没尾的转圈；
                    文案收进胶囊压在封面上。自动重试耗尽的错误态再给一个
                    手动重试入口——用户不该只能眼看黑屏干着急。 */}
                <div className="flex flex-col items-center gap-3">
                  <span className="flex items-center gap-2 rounded-full bg-black/60 px-4 py-1.5 text-xs text-white/85 backdrop-blur-sm">
                    {!error && <Loader2 className="size-3.5 animate-spin" aria-hidden />}
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
                  </span>
                  {error && (
                    <Button
                      size="sm"
                      variant="outline"
                      data-wheel-block
                      onClick={(e) => {
                        e.stopPropagation();
                        retryOnline();
                      }}
                    >
                      {t('player.retry')}
                    </Button>
                  )}
                </div>
              </div>
            </>
          )}
          </div>
        </div>

        {/* 页面级杂物的浮层化：错误条 / 连播开关压在画面顶部**靠左**排布。
            顶部居中是沉浸流的分类 tab 栏（topChrome 插槽），左上不能太宽，
            不然把 tab 挡死——快捷键提示从常驻位撤下（错误时才占这个位置）。 */}
        <div
          data-wheel-block
          className={cn(
            'absolute left-3 top-3 z-20 flex items-center gap-2',
            'transition-opacity duration-300',
            chromeShown ? 'opacity-100' : 'pointer-events-none opacity-0',
          )}
        >
          {error && (
            <span className="rounded-full border border-red-500/30 bg-red-950/90 px-3 py-1 text-xs text-red-200">
              {error}
            </span>
          )}
          {inBinge && (
            <div className="flex items-center gap-2 rounded-full bg-primary px-3 py-1 text-xs font-medium text-primary-foreground shadow-sm">
              <span>{t('player.bingeActive')}</span>
              <button
                type="button"
                onClick={() => setBinge(null)}
                className="cursor-pointer underline-offset-2 opacity-80 hover:underline hover:opacity-100"
              >
                {t('player.bingeExit')}
              </button>
            </div>
          )}
          <div className="flex shrink-0 items-center gap-3 rounded-full bg-black/45 px-3 py-1 backdrop-blur-sm">
            <div className="flex items-center gap-1.5">
              <Switch
                id="auto-next"
                checked={autoNext}
                disabled={settingsPending}
                onCheckedChange={(v) => patchSettings({ autoNextEpisode: v })}
              />
              <Label htmlFor="auto-next" className="text-xs text-white/85">
                {t('player.autoNext')}
              </Label>
            </div>
            <div className="flex items-center gap-1.5">
              <Switch
                id="auto-delete"
                checked={autoDelete}
                disabled={settingsPending}
                onCheckedChange={(v) => patchSettings({ autoDeleteAfterPlay: v })}
              />
              <Label htmlFor="auto-delete" className="text-xs text-white/85">
                {t('player.autoDelete')}
              </Label>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * 封面占位图（裂图自愈）。
 *
 * 源图可能是 HEIC（部分 WebView 渲染不了），onError 后整层退场，不留一个
 * 破图标压在画面上。`hidden` 是「视频已出画」：淡出而非卸载，切下一部剧时
 * key 随 src 变化重挂载，broken/透明度状态自然归零。
 */
function CoverBackdrop({ src, hidden }: { src: string; hidden: boolean }) {
  const [broken, setBroken] = useState(false);
  if (broken) return null;
  return (
    <img
      src={src}
      alt=""
      aria-hidden
      onError={() => setBroken(true)}
      className={cn(
        'pointer-events-none absolute inset-0 z-[5] size-full object-cover object-top brightness-[0.55]',
        'transition-opacity duration-500',
        hidden ? 'opacity-0' : 'opacity-100',
      )}
    />
  );
}

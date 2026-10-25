import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Button } from '@/components/ui/button';
import { ErrorState } from '@/components/common/error-state';
import { Empty, EmptyContent, EmptyHeader, EmptyTitle } from '@/components/ui/empty';
import { PlayerView } from '@/features/player/components/player-page';
import { play } from '@/service/commands';
import {
  keys,
  useAccount,
  useBookshelf,
  useFeed,
  useFeedTabs,
  useLocalHistory,
  usePrefetchSeriesEpisodes,
  useSeriesEpisodes,
  useSeriesProgress,
  useSettings,
  useWatchHistory,
} from '@/service/queries';
import { enhancedCover, isRenderableCover } from '@/utils/cover';
import { resolveResumeEntry } from '@/utils/resume-entry';
import { usePlayerStore } from '@/stores/player';
import { t } from '@/locales';
/** 推荐流的「换一批」过期阈值（毫秒）：恢复游标后流数据比这更旧就整体
 *  重开会话。 */
const FEED_STALE_MS = 5 * 60_000;
/** 首帧未出画时兜底预取下一部流前的等待（毫秒）：给当前剧起播让路。 */
const PREFETCH_FALLBACK_DELAY_MS = 8_000;
import type { Series } from '@/service/schema';
import { RECOMMEND_TAB_TYPE, buildTabList } from './home-tabs';
import { TopBarTabs } from './top-bar-tabs';
import { BootSplash } from './boot-splash';

/**
 * 首页：沉浸式播放器流（官方客户端同款形态）。
 *
 * 视频**铺满整页**，顶部居中是分类 tab 胶囊——列表由服务端下发
 * （bookmall/tab 响应 tab_item[]：tab_type/标题/顺序，拉取前/失败回落
 * 本地三件套；关注一律不显示，追更由收藏页承载），内容随服务端时间桶
 * 轮换。滚轮 / ↑↓ 在当前源内切上一部/下一部剧；剧名/集数/简介叠加在
 * 画面左下，鼠标静止 3 秒连同互动栏一起淡出。
 * （推荐 tab 头部已种入云端历史+书架，「你的内容」不缺位。）
 *
 * 切换要快：进入当前剧就预取相邻剧的分集档案；当前剧首帧一出画再预取
 * 下一部的流（后端瞄准续播集）——真正切过去时只剩取流时间。
 */

/** 源内一条：沉浸流只消费这几个身份字段。 */
interface StreamItem {
  seriesId: string;
  title: string;
  cover: string;
  /** 横版封面（信息流才有）：全屏窗口的占位图用它，竖版 3:4 拉伸铺满横屏会糊。 */
  horizCover: string;
  /** 热度文本（信息流条目带，剧名上方展示） */
  heatText?: string;
  /** 季角标（「第1季」；剧名前 pill） */
  seasonTag?: string;
  /** 官方运营角标（「新剧/爆剧/红果首发」；剧名前标签） */
  badge?: string;
}

const SOURCE_KEY = 'hongguo.feedSource';

function readSource(): string {
  try {
    const v = window.localStorage.getItem(SOURCE_KEY);
    if (v) return v;
  } catch {
    // 读不到就用默认
  }
  return RECOMMEND_TAB_TYPE;
}

export function HomePage() {
  // 一次性清掉旧推荐流的持久化键（landpage 时代的口味统计/入口游标，
  // 2026-10-08 迁到书城 cell 换一换后不再读写，留着只是脏数据）
  useEffect(() => {
    for (const k of ['hongguo.typeStats', 'hongguo.feedEntry']) {
      try {
        window.localStorage.removeItem(k);
      } catch {
        // 清不掉不影响功能
      }
    }
  }, []);

  const [source, setSource] = useState<string>(readSource);
  /** 每源独立游标：切 tab 回来还在上次刷到的位置。 */
  const [indexes, setIndexes] = useState<Record<string, number>>({});
  /** 「在播不动」闸门：换一批（重开会话）后的新列表通常不含在播剧，
   *  把它钉在最前保住在播画面。跨 tab 切换必须关闸——否则在播剧占住
   *  新源的第 0 位，播放器目标永远不换，看起来「点了没反应」；关闸后
   *  新源首批一到，当前剧就落到该 tab 游标处，切 tab 即换片。
   *  挂载默认开闸：从别的菜单回首页时在播剧往往不在信息流里，钉住它
   *  才能「切菜单回来不换视频」。 */
  const [pinHold, setPinHold] = useState(true);

  const feed = useFeed(source);
  const prefetchEpisodes = usePrefetchSeriesEpisodes();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const playingId = usePlayerStore((s) => s.seriesId);
  // store 目标的集号（0 = 只点名了剧，集号待本组件的起点决策补上）
  const storeVidIndex = usePlayerStore((s) => s.vidIndex);
  const setResumeHint = usePlayerStore((s) => s.setResumeHint);
  /** 回首页恢复是否已尝试（一次性；见下方渲染期恢复块）。 */
  const [restored, setRestored] = useState(false);

  // 登录后的「你的内容」种子：观看历史 + 书架（追更）排在推荐流最前
  // ——同一个号打开就是你的剧，对齐官方客户端（首页就是书架/续看驱动）。
  // 历史/书架条目没有横版封面，标题等 currentSeries 解析出来自然显示。
  const { data: account } = useAccount();
  // 历史源双模式（与历史页同款判定）：登录读云端（跨端续播），匿名读
  // 本地记录。云端接口匿名也不报错，会回设备级历史行——那是匿名期快速
  // 测试留下的 vi=0/pos=0 垃圾，拿它当续播依据就是「每次都回第 1 集」
  // 的根源；本应用的真进度在本地历史/播放档案里。
  const loggedIn = !!account;
  const cloudHistory = useWatchHistory(loggedIn);
  const localHistory = useLocalHistory(!loggedIn);
  const historyItems = loggedIn ? cloudHistory.data?.items : localHistory.data;
  const historyPending = loggedIn ? cloudHistory.isPending : localHistory.isPending;
  // 顶栏 tab 列表（服务端下发；拉取前/失败回落三件套，见 buildTabList）
  const { data: serverTabs } = useFeedTabs();
  const homeTabs = useMemo(() => buildTabList(serverTabs), [serverTabs]);
  // 服务端列表到手后校验持久化的选中 tab：被服务端撤掉（或登录态变化后
  // 失去数据面的关注）就重置回推荐 tab，别停在拉不到数据的死 tab 上。
  // 回落三件套期间不校验——持久化 id 可能是服务端新 tab，等真列表说话。
  // 渲染期 setState（官方 adjust 模式，与下方游标恢复同款）：条件在重渲染
  // 后自然为假，不会循环。
  if (serverTabs?.length && !homeTabs.some((t) => t.id === source)) {
    setSource(RECOMMEND_TAB_TYPE);
    try {
      window.localStorage.setItem(SOURCE_KEY, RECOMMEND_TAB_TYPE);
    } catch {
      // 存不进只影响下次启动
    }
  }
  const { data: bookshelf } = useBookshelf();
  const seedItems = useMemo<StreamItem[]>(() => {
    const seen = new Set<string>();
    const out: StreamItem[] = [];
    for (const h of historyItems ?? []) {
      if (!h.seriesId || seen.has(h.seriesId)) continue;
      seen.add(h.seriesId);
      out.push({ seriesId: h.seriesId, title: h.title, cover: h.cover, horizCover: '' });
    }
    for (const b of bookshelf ?? []) {
      if (!b.seriesId || seen.has(b.seriesId)) continue;
      seen.add(b.seriesId);
      out.push({ seriesId: b.seriesId, title: '', cover: '', horizCover: '' });
    }
    return out;
  }, [historyItems, bookshelf]);

  // 三 tab 同一数据面，统一成 StreamItem[]。
  // useMemo 稳定身份：数组每渲染新建会让依赖它的 effect/哨兵反复重跑。
  const feedItems = feed.items;
  // 「你的内容」种子只属于推荐 tab：漫剧/真人是服务端类型 tab，头一条必须
  // 是该类型的剧——种子(最近看过)排头会把刚看完的那部顶在前面，点 tab
  // 看起来毫无反应(实测:刚看完动漫点真人,头部还是那部动漫)。
  const items: StreamItem[] = useMemo(() => {
    const activeSeeds = source === RECOMMEND_TAB_TYPE ? seedItems : [];
    return [
      ...activeSeeds,
      ...feedItems
        .filter(
          (i, idx, arr) =>
            // 会话轮换后跨页偶发同条目，连种子一起按 seriesId 去重
            !activeSeeds.some((seed) => seed.seriesId === i.seriesId) &&
            arr.findIndex((x) => x.seriesId === i.seriesId) === idx,
        )
        .map((i) => ({
          seriesId: i.seriesId,
          title: i.title,
          cover: i.cover,
          horizCover: i.horizCover,
          heatText: i.heatText,
          seasonTag: i.seasonTag,
          badge: i.badge,
        })),
    ];
  }, [source, seedItems, feedItems]);

  // 尾部翻页能力（同会话 session 游标续拉）
  const hasMore = feed.hasMore;
  const isFetchingMore = feed.isFetchingMore;
  const loadMore = feed.loadMore;

  // ---- 回首页换一批（>5 分钟过期）但在播剧不动 ----
  // 恢复（游标对回 store 目标剧）落定后，若推荐流已过期：重开会话取新
  // 一批（reset 语义，原地重放只会是旧会话），游标
  // 归零；在播剧由下方置顶钉住继续播，滚下去才是新一批。
  const queryClient = useQueryClient();
  useEffect(() => {
    if (!restored || !playingId) return;
    if (feed.isRefreshing || feed.isLoading) return;
    if (!(feed.dataUpdatedAt > 0 && Date.now() - feed.dataUpdatedAt > FEED_STALE_MS)) return;
    feed.restart();
    // 钉住在播 + 游标归零一起放微任务：effect 体里同步 setState 会触发
    // react-hooks 告警
    queueMicrotask(() => {
      setPinHold(true);
      setIndexes((prev) => ({ ...prev, [source]: 0 }));
    });
  }, [restored, playingId, feed, source, setIndexes]);

  // 在播剧置顶钉：换一批后的新列表里通常没有它，「在播不动」靠把它插
  // 在最前（标题/封面从档案缓存取，缺失给占位——起播走 store 目标，不受
  // 影响）。只服务换一批/回首页场景，跨 tab 切换关闸（见 pinHold）。
  const playingPin = useMemo(() => {
    if (!pinHold || !playingId || items.some((i) => i.seriesId === playingId)) return null;
    const cached = queryClient.getQueryData<Series>(keys.seriesEpisodes(playingId));
    return {
      seriesId: playingId,
      title: cached?.title ?? '',
      cover: cached?.cover ?? '',
      horizCover: '',
      heatText: undefined,
      seasonTag: undefined,
      badge: undefined,
    };
  }, [pinHold, playingId, items, queryClient]);
  const displayItems = useMemo(
    () => (playingPin ? [playingPin, ...items] : items),
    [playingPin, items],
  );

  // 外部点名消费（详情/历史/搜索卡片、选集切季、剧终接下一季）：游标对到
  // 目标剧——流里有就对过去，没有就 pinHold + 落到钉位（第 0 位）。
  // 数据面未就绪就先不消费，等下一次渲染补齐。渲染期 setState（官方
  // adjust 模式，与下方游标恢复同款）：条件在重渲染后自然为假，不会循环；
  // 一次性消费：不与用户后续滚动互相拽（信息流自己的 setTarget 不带信号）。
  const gotoNonce = usePlayerStore((s) => s.gotoNonce);
  const [consumedGoto, setConsumedGoto] = useState(gotoNonce);
  if (consumedGoto !== gotoNonce && playingId && displayItems.length > 0) {
    setConsumedGoto(gotoNonce);
    setPinHold(true);
    const idx = displayItems.findIndex((i) => i.seriesId === playingId);
    setIndexes((prev) => ({ ...prev, [source]: Math.max(idx, 0) }));
  }

  // 游标越界(榜单/新剧源切换)夹到已加载尾部，后台翻页跟上。
  const rawIndex = indexes[source] ?? 0;
  const index = Math.min(rawIndex, Math.max(0, displayItems.length - 1));
  const current = displayItems[index];
  const currentId = current?.seriesId;

  // 信息流条目的展示标记（热度/季角标）：档案接口没有这几个字段，
  // 只能从流条目上带给播放器叠加层。
  const overlayMeta = useMemo(
    () =>
      current
        ? {
            heatText: current.heatText,
            seasonTag: current.seasonTag,
            badge: current.badge,
          }
        : undefined,
    [current],
  );

  /**
   * 当前剧的档案：走 `get_series_episodes`（本地命中秒回，缺失才回落解析），
   * **不走 resolve_series**——那是每次都打网络的解析，预取的缓存它吃不到，
   * 滚轮切剧会在「正在准备」上白等一拍。预取 effect 已经把下一部剧的
   * 分集档案灌进同一份缓存，这里直接命中。
   */
  const seriesQuery = useSeriesEpisodes(currentId ?? '');
  const currentSeries = seriesQuery.data;
  // 两路进度源（官方 App「跟上历史进度」同款）：
  // - 本地播放档案：本应用 5 秒一写的真值（毫秒级 IPC）
  // - 云端观看历史：跨客户端（官方 App 看过的也算），种子区
  //   已在用同一查询，这里共享缓存
  const progressQuery = useSeriesProgress(currentId ?? '');
  const localProgress = progressQuery.data;

  // 回首页恢复（切菜单回来不换视频）：store 目标剧还在信息流里 → 游标
  // 对回那部剧。信息流游标是本组件 state，切菜单重挂载会归零，不恢复的
  // 话起点决策会把目标换成第一部、正在看的视频被顶掉。
  // 渲染期 setState（官方 adjust 模式）：置位后本组件立即以新游标重渲染，
  // effect 只在最终提交后跑一次——起点决策自然对准恢复的那部剧，无需
  // 额外闩锁。只恢复一次：之后用户滚走再滚回，不再拽游标。
  if (!restored && items.length > 0) {
    setRestored(true);
    if (playingId) {
      const idx = items.findIndex((i) => i.seriesId === playingId);
      if (idx > 0) setIndexes((prev) => ({ ...prev, [source]: idx }));
    }
  }

  // 档案 + 两路进度源就绪 → 定起点。看过 >3 秒就续播那一集的那个位置，
  // 不看片尾（2026-10-09 用户口径）。云端历史的集内位置塞进 resumeHint：
  // 播放器起播在本地没有该集播放档案（resumeAt=0）时用它 seek——跨客户
  // 端续播连进度都对上。
  //
  // 让位条件 = 目标剧一致**且**集号已指定（>0）：正在看的时候历史刷新/
  // 进度上报不能把目标拽走；而追更/收藏/预约这些只点名剧的入口
  // （vidIndex=0）要靠这里跑起点决策补上集号（issue #11）。滚走再滚
  // 回来时会重新对号本地档案，接的是你离开时的那集。
  // 恢复游标的渲染期 setState 让 effect 只在游标生效后的提交上跑一次，
  // 恢复在途（档案未跟上）时 currentSeries 为空自然空转，无需额外闩锁。
  useEffect(() => {
    if (!currentSeries) return;
    if (playingId === currentSeries.seriesId && (storeVidIndex ?? 0) > 0) return;
    if (progressQuery.isPending || historyPending) return;

    // >3 秒都续播（不看片尾）：本地档案 vs 云端历史取新者（共享决策，
    // 与详情页「继续看」/后端 resolve_entry 同口径）
    const entry = resolveResumeEntry(
      localProgress,
      historyItems?.find((i) => i.seriesId === currentSeries.seriesId),
    );

    setTarget(currentSeries.seriesId, entry?.index ?? 1);
    if (entry && entry.positionMs > 0) {
      setResumeHint({
        seriesId: currentSeries.seriesId,
        vidIndex: entry.index,
        positionMs: entry.positionMs,
      });
    }
  }, [
    currentSeries,
    playingId,
    storeVidIndex,
    localProgress,
    progressQuery.isPending,
    historyItems,
    historyPending,
    setTarget,
    setResumeHint,
  ]);

  // 占位封面：横版优先（竖版 3:4 被 object-cover 拉满横屏窗口=整屏发糊）；
  // 再挑 WebView 渲染得了的 URL。eagerProxy：HEIC 源不等 webp 网络请求，
  // 立即给本地转码代理——切剧瞬间的全屏占位黑一秒都是「卡顿感」的来源
  const rawCover = current?.horizCover || current?.cover || '';
  const webCover = enhancedCover(rawCover);
  const coverForPlayer = current
    ? (webCover ?? (isRenderableCover(rawCover) ? rawCover : undefined))
    : undefined;

  // 预取相邻剧（下一部为主、上一部兜底回退）的分集档案：换剧时 resolve 秒回
  const nextItem = items[index + 1];
  const prevItem = items[index - 1];
  useEffect(() => {
    if (nextItem) prefetchEpisodes(nextItem.seriesId);
  }, [nextItem, prefetchEpisodes]);
  useEffect(() => {
    if (prevItem) prefetchEpisodes(prevItem.seriesId);
  }, [prevItem, prefetchEpisodes]);

  // 预取下一部剧续播集的**流**（取流表 + 渐进填充头部）：滚过去时缓存已
  // 就绪，首帧只等头部数据。触发时机 = 当前剧首帧一出画（PlayerView 回
  // 调）——在播那一路站稳了，带宽才让给铺垫；固定延迟会让下一部的就绪
  // 时间跟着延迟走，快滚时预取干脆来不及。起播被卡住（自动播被 WebView
  // 拦、网络慢）时首帧信号不来，兜底定时照发——慢是慢，不能不备。
  // 广度 = 预取档位（0=全关）：后续 B 部每部预取入口集（issue #11：只铺
  // 紧邻下一部时快滑 3-5 部全是冷启动）。同一部剧只取一次；失败不算数，
  // 下次再成为「后续部」时允许重试。
  const { data: settings } = useSettings();
  const prefetchBreadth = settings?.prefetchEpisodes ?? 3;
  const streamPrefetched = useRef(new Set<string>());
  const prefetchStream = useCallback((id: string) => {
    if (streamPrefetched.current.has(id)) return;
    streamPrefetched.current.add(id);
    void play.prefetch(id).catch(() => streamPrefetched.current.delete(id));
  }, []);
  const upcomingIds = useMemo(
    () =>
      items
        .slice(index + 1, index + 1 + prefetchBreadth)
        .map((it) => it.seriesId)
        .filter(Boolean),
    [items, index, prefetchBreadth],
  );
  const upcomingKey = upcomingIds.join(',');
  /** 当前剧是否已出画：PlayerView 首帧回调记下是哪部剧出的画（渲染期
      对号，切剧瞬间自动回到「未出画」，无需 effect 追着清）。 */
  const [firstFrameFor, setFirstFrameFor] = useState('');
  const stageLive = current !== undefined && firstFrameFor === current.seriesId;
  useEffect(() => {
    if (!stageLive) return;
    upcomingIds.forEach((id) => prefetchStream(id));
    // upcomingIds 随 items/游标重算，用拼串做稳定依赖，避免每帧重发
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stageLive, upcomingKey, prefetchStream]);
  useEffect(() => {
    if (stageLive || upcomingIds.length === 0) return;
    const timer = setTimeout(
      () => upcomingIds.forEach((id) => prefetchStream(id)),
      PREFETCH_FALLBACK_DELAY_MS,
    );
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stageLive, upcomingKey, prefetchStream]);

  // 快滑到源尾部时预取下一页；游标越界时也靠它链式拉齐
  useEffect(() => {
    if (loadMore && hasMore && !isFetchingMore && items.length - index <= 3) loadMore();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, index, items.length, hasMore, isFetchingMore]);

  // 依赖里的 source/长度每变一次本来就要新函数，useCallback 省不了重建。
  // 别按「React Compiler 已启用」的口径写注释——本项目没装编译器插件。
  const step = (dir: 1 | -1) => {
    setIndexes((prev) => {
      const max = Math.max(0, items.length - 1);
      return { ...prev, [source]: Math.min(Math.max((prev[source] ?? 0) + dir, 0), max) };
    });
  };

  // 分类 tab 现已 portal 进 AppShell 顶栏（见 TopBarTabs），不再悬浮画面。
  // 样式与排行榜/新剧的内容 tab 同一套主题语义色——全应用一个 tab 语言，
  // 自动跟随亮暗主题。**重复点当前 tab = 换一批**（「再刷一组」的最直接
  // 入口）：重开会话取服务端当前时间桶的新一批，在播剧钉住不动。
  // 切到**别的** tab 则关掉在播钉：新源首批一到，当前剧即落到该 tab
  // 游标处并接管播放——切 tab 就换片，而不是停在上一 tab 的剧上等滚动。
  const pickTab = (id: string) => {
    if (id === source) {
      feed.restart();
      setPinHold(true);
      setIndexes((prev) => ({ ...prev, [id]: 0 }));
      return;
    }
    setPinHold(false);
    setSource(id);
    try {
      window.localStorage.setItem(SOURCE_KEY, id);
    } catch {
      // 存不进只影响下次启动
    }
  };

  const isLoading = feed.isLoading;
  const loadError = feed.error;
  // 重试 = 重拉当前会话（失败重试不是换一批，keep 语义分离）
  const retrySource = () => feed.refresh();

  // 冷启动首批在拉：**不整页换骨架屏**——store 里还播着上一部（或冷启动
  // 时连 store 目标都还没有），前者主树继续渲染 PlayerView 顶住画面 +
  // 顶部细进度条，后者给品牌化启动画面（骨架黑砖在暗色下被感知成黑屏）。
  if (isLoading && items.length === 0 && !playingId) {
    return (
      <div className="relative h-full min-h-0 overflow-hidden">
        <TopBarTabs tabs={homeTabs} source={source} onPick={pickTab} />
        <BootSplash />
      </div>
    );
  }
  // 分集档案解析失败（含 Rust 重启后 invoke 挂起转超时）：必须给重试入口，
  // 静默停在「还没有选择剧集」就是用户反复看到的那个神秘界面
  if (seriesQuery.isError && current) {
    return (
      <ErrorState
        message={t('feed.loadFailed')}
        detail={String(seriesQuery.error)}
        onRetry={() => void seriesQuery.refetch()}
      />
    );
  }
  if (loadError && items.length === 0) {
    return (
      <ErrorState
        message={t('feed.loadFailed')}
        detail={loadError}
        onRetry={() => void retrySource()}
      />
    );
  }
  // 首批在拉时不是空态：画面继续播上一次的剧，拉到位自动过去
  if (!current && items.length === 0 && !isLoading) {
    return (
      <Empty className="py-16">
        <EmptyHeader>
          <EmptyTitle className="text-muted-foreground text-sm font-normal">
            {t('home.empty')}
          </EmptyTitle>
        </EmptyHeader>
        <EmptyContent>
          <Button variant="outline" size="sm" onClick={() => void retrySource()}>
            {t('feed.retry')}
          </Button>
        </EmptyContent>
      </Empty>
    );
  }

  // 切 tab 后首批在拉（旧画面继续播）：亮顶部细进度条给「正在切换」
  // 的反馈——没有它就是「点了没反应」（用户实测反馈）。
  const sourceBooting = feed.isLoading;

  return (
    // overflow-hidden + 绝对定位铺满：AppShell 的 main 是 overflow-y-auto
    // （别的页面靠它滚），沉浸流内部任何 1px 超高都会冒出一条页面滚动条，
    // 还会把滚轮切剧吃掉——这里整个锁死在视口内。
    <div className="relative h-full min-h-0 overflow-hidden">
      {/* 分类 tab 进顶栏（薄行形态，画面上不再有悬浮 tab 层） */}
      <TopBarTabs tabs={homeTabs} source={source} onPick={pickTab} />
      {sourceBooting && (
        <div className="pointer-events-none absolute inset-x-0 top-0 z-50 h-0.5">
          <div className="hg-loadbar-track">
            <div className="bg-primary hg-loadbar" />
          </div>
        </div>
      )}
      <div className="absolute inset-0">
        {/* 播放器铺满整页；滚轮/↑↓ 在当前 tab 源内切上一部/下一部剧；
            封面给播放器做占位——切剧的取流间隙显示下一部剧的封面而非黑屏 */}
        <PlayerView
          onWheelStep={step}
          coverUrl={coverForPlayer}
          overlayMeta={overlayMeta}
          onFirstFrame={() => setFirstFrameFor(current?.seriesId ?? '')}
        />
      </div>
    </div>
  );
}

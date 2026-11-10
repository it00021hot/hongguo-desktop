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
import { withPinnedPlaying } from '@/utils/pinned-playing';
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
import { decideStartPoint } from './start-point';
import { TopBarTabs } from './top-bar-tabs';
import { BootSplash } from './boot-splash';

/**
 * 首页：沉浸式播放器流（官方客户端同款形态）。视频铺满整页，顶部居中分类 tab 胶囊
 * （服务端下发，拉取前/失败回落本地三件套；关注一律不显示）。滚轮/↑↓ 在当前源内切上一部/下一部剧。
 * 切换要快：进当前剧就预取相邻剧的分集档案；首帧一出画再预取下一部的流（后端瞄准续播集）。
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
  // 迁到书城 cell 换一换后不再读写，留着只是脏数据）
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
  /**
   * 「在播不动」闸门：换一批后的新列表通常不含在播剧，钉在最前保住在播画面。
   * 跨 tab 切换必须关闸（否则在播剧占住新源第 0 位，播放器目标永远不换）；
   * 挂载默认开闸——回首页时在播剧往往不在信息流里，钉住才能「切菜单回来不换视频」。
   */
  const [pinHold, setPinHold] = useState(true);

  const feed = useFeed(source);
  const prefetchEpisodes = usePrefetchSeriesEpisodes();
  const setTarget = usePlayerStore((s) => s.setTarget);
  const playingId = usePlayerStore((s) => s.seriesId);
  const bingeSeriesId = usePlayerStore((s) => s.bingeSeriesId);
  // store 目标的集号（0 = 只点名了剧，集号待本组件的起点决策补上）
  const storeVidIndex = usePlayerStore((s) => s.vidIndex);
  const setResumeHint = usePlayerStore((s) => s.setResumeHint);
  /** 回首页恢复是否已尝试（一次性；见下方渲染期恢复块）。 */
  const [restored, setRestored] = useState(false);

  // 登录后的「你的内容」种子：观看历史 + 书架（追更）排在推荐流最前（首页即书架/续看驱动）
  const { data: account } = useAccount();
  // 历史源双模式：登录读云端（跨端续播），匿名读本地记录——云端匿名回的设备级
  // 历史行是匿名期快速测试留下的垃圾（vi=0/pos=0），拿它当续播依据就是「每次都回第 1 集」的根源
  const loggedIn = !!account;
  const cloudHistory = useWatchHistory(loggedIn);
  const localHistory = useLocalHistory(!loggedIn);
  const historyItems = loggedIn ? cloudHistory.data?.items : localHistory.data;
  const historyPending = loggedIn ? cloudHistory.isPending : localHistory.isPending;
  // 顶栏 tab 列表（服务端下发；拉取前/失败回落三件套，见 buildTabList）
  const { data: serverTabs } = useFeedTabs();
  const homeTabs = useMemo(() => buildTabList(serverTabs), [serverTabs]);
  // 服务端列表到手后校验持久化的选中 tab：被服务端撤掉就重置回推荐 tab；
  // 回落三件套期间不校验（持久化 id 可能是新 tab，等真列表说话）。渲染期 setState，条件在重渲染后自然为假
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
  // 「你的内容」种子只属于推荐 tab：漫剧/真人是服务端类型 tab，种子排头会把刚看完的顶在前面（点 tab 像没反应）
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

  // 回首页换一批（>5 分钟过期）但在播剧不动：重开会话取新一批（reset 语义），游标归零；
  // 在播剧由置顶钉住继续播
  const queryClient = useQueryClient();
  useEffect(() => {
    if (!restored || !playingId) return;
    if (feed.isRefreshing || feed.isLoading) return;
    if (!(feed.dataUpdatedAt > 0 && Date.now() - feed.dataUpdatedAt > FEED_STALE_MS)) return;
    feed.restart();
    // 钉住 + 游标归零放微任务：effect 体里同步 setState 会触发 react-hooks 告警
    queueMicrotask(() => {
      setPinHold(true);
      setIndexes((prev) => ({ ...prev, [source]: 0 }));
    });
  }, [restored, playingId, feed, source, setIndexes]);

  // 在播剧置顶钉：换一批/流过期重开后的新列表通常没有它，**无条件钉**（游标是数组下标，
  // 不钉换一批后游标 0 就指到别的剧，起点决策把播放目标拽走——机制见 withPinnedPlaying）。
  // 只服务换一批/回首页场景，跨 tab 切换关闸（见 pinHold）。
  const displayItems = useMemo(
    () =>
      withPinnedPlaying(items, pinHold, playingId, () => {
        if (!playingId) return null;
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
      }),
    [items, pinHold, playingId, queryClient],
  );

  // 外部点名消费（详情/历史/搜索卡片、切季、剧终接力）：游标对到目标剧——流里有就对过去，
  // 没有就 pinHold + 落钉位；数据面未就绪先不消费。渲染期 setState；一次性消费，不与用户滚动互相拽。
  const gotoNonce = usePlayerStore((s) => s.gotoNonce);
  // 已消费 nonce 以 store 为准：外部点名先于首页挂载发生，useState 拿当前 nonce
  // 初始化会把信号在挂载帧判成「已消费」；store 里的值跨挂载存活
  const [consumedGoto, setConsumedGoto] = useState(() => usePlayerStore.getState().gotoConsumed);
  useEffect(() => {
    usePlayerStore.getState().markGotoConsumed(consumedGoto);
  }, [consumedGoto]);
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
   * 不走 resolve_series——那是每次打网络的解析，预取的缓存它吃不到。
   */
  const seriesQuery = useSeriesEpisodes(currentId ?? '');
  const currentSeries = seriesQuery.data;
  // 两路进度源：本地播放档案（5 秒一写真值）+ 云端观看历史（跨客户端），种子区已在用同一查询
  const progressQuery = useSeriesProgress(currentId ?? '');
  const localProgress = progressQuery.data;

  // 回首页恢复（切菜单回来不换视频）：store 目标剧还在流里 → 游标对回那部剧
  // （游标是组件 state，重挂载会归零，不恢复则起点决策把目标换成第一部、正在看的视频被顶掉）。
  // 渲染期 setState 只恢复一次：之后用户滚走再滚回不再拽游标。
  if (!restored && items.length > 0) {
    setRestored(true);
    if (playingId) {
      // 在播剧钉住时它在 displayItems[0]，idx=0 不设（游标默认就是 0）
      const idx = displayItems.findIndex((i) => i.seriesId === playingId);
      if (idx > 0) setIndexes((prev) => ({ ...prev, [source]: idx }));
    }
  }

  // 档案 + 两路进度源就绪 → 定起点。看过 >3 秒就续播那一集的那个位置，
  // 不看片尾。云端历史的集内位置塞进 resumeHint：
  // 播放器起播在本地没有该集播放档案（resumeAt=0）时用它 seek——跨客户
  // 端续播连进度都对上。
  //
  // 让位条件 = 目标剧一致**且**集号已指定（>0）：正在看的时候历史刷新/
  // 进度上报不能把目标拽走；而追更/收藏/预约这些只点名剧的入口
  // （vidIndex=0）要靠这里跑起点决策补上集号（issue #11）。滚走再滚
  // 回来时会重新对号本地档案，接的是你离开时的那集。
  // 恢复游标的渲染期 setState 让 effect 只在游标生效后的提交上跑一次，
  // 恢复在途（档案未跟上）时 currentSeries 为空自然空转，无需额外闩锁。
  // 判定与动作拆在 start-point.ts（纯函数，事故即复现测试）。游标是否
  // 由用户/点名动作改写（vs 列表移位顶掉游标处内容）是保护分支的判据，
  // 记在 ref：idxChanged 逐拍重算的话，「档案/进度未就绪」的早退会把它
  // 吃掉，晚一拍送达的决策误判成「游标没动」，用户刚滚的轮被保护弹回。
  const lastIndexRef = useRef(-1);
  const cursorMovedRef = useRef(false);
  useEffect(() => {
    if (!currentSeries) return;
    if (lastIndexRef.current !== index) {
      lastIndexRef.current = index;
      cursorMovedRef.current = true;
    }
    if (progressQuery.isPending || historyPending) return;

    const decision = decideStartPoint({
      pinHold,
      cursorMoved: cursorMovedRef.current,
      playingId,
      cursorSeriesId: currentSeries.seriesId,
      storeVidIndex: storeVidIndex ?? 0,
      index,
      items,
    });
    // 凡到达决策点（含 wait=起点已定），游标信号都已兑现
    cursorMovedRef.current = false;
    if (decision.kind === 'wait') return;

    // 在播保护：对齐游标回在播剧，绝不拽播放目标（保护分支的轮回机制
    // 见 start-point.ts）——纯 UI 写，一轮收敛。
    if (decision.kind === 'protect') {
      setIndexes((prev) => ({ ...prev, [source]: decision.anchorIndex }));
      return;
    }

    // >3 秒都续播（不看片尾）：本地档案 vs 云端历史取新者（共享决策，
    // 与详情页「继续看」/后端 resolve_entry 同口径）
    const entry = resolveResumeEntry(
      localProgress,
      historyItems?.find((i) => i.seriesId === decision.seriesId),
    );

    // 主动切剧即结束「在播保护」——但只在游标剧在未钉住的列表里时关闸（decision.releasePin）：
    // 外部点名的目标只以钉住占位存在，关闸它就从列表消失、起点决策把播放目标抢走（「搜索点开播错剧」的根源）
    if (decision.releasePin) setPinHold(false);
    // 撤钉会让钉位之前的内容左移一格，游标按内容重锚到同一步剧
    const anchorIndex = decision.anchorIndex;
    if (anchorIndex !== null) {
      setIndexes((prev) => ({ ...prev, [source]: anchorIndex }));
    }
    setTarget(decision.seriesId, entry?.index ?? 1);
    if (entry && entry.positionMs > 0) {
      setResumeHint({
        seriesId: decision.seriesId,
        vidIndex: entry.index,
        positionMs: entry.positionMs,
      });
    }
  }, [
    currentSeries,
    index,
    playingId,
    pinHold,
    items,
    source,
    storeVidIndex,
    localProgress,
    progressQuery.isPending,
    historyItems,
    historyPending,
    setTarget,
    setResumeHint,
    setPinHold,
  ]);

  // 占位封面横版优先（竖版拉满横屏会糊）；HEIC 源立即给本地转码代理（切剧瞬间黑一秒都是「卡顿感」）
  const rawCover = current?.horizCover || current?.cover || '';
  const webCover = enhancedCover(rawCover);
  const coverForPlayer = current
    ? (webCover ?? (isRenderableCover(rawCover) ? rawCover : undefined))
    : undefined;

  // 预取相邻剧（下一部为主、上一部兜底回退）的分集档案：换剧时 resolve 秒回。
  // index 是 displayItems 的游标(钉住时两表错位 1),必须同表取相邻
  const nextItem = displayItems[index + 1];
  const prevItem = displayItems[index - 1];
  useEffect(() => {
    if (nextItem) prefetchEpisodes(nextItem.seriesId);
  }, [nextItem, prefetchEpisodes]);
  useEffect(() => {
    if (prevItem) prefetchEpisodes(prevItem.seriesId);
  }, [prevItem, prefetchEpisodes]);

  // 预取下一部剧续播集的流：触发时机 = 当前剧首帧一出画（在播站稳了带宽才让给铺垫）；
  // 起播被卡住时兜底定时照发。广度 = 预取档位；同一部只取一次，失败不算数下次重试
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
      bingeSeriesId === currentId
        ? []
        : displayItems
            .slice(index + 1, index + 1 + prefetchBreadth)
            .map((it) => it.seriesId)
            .filter(Boolean),
    [bingeSeriesId, currentId, displayItems, index, prefetchBreadth],
  );
  const upcomingKey = upcomingIds.join(',');
  /** 当前剧是否已出画：PlayerView 首帧回调记下是哪部剧出的画（渲染期对号，切剧瞬间自动回到「未出画」）。 */
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
    if (loadMore && hasMore && !isFetchingMore && displayItems.length - index <= 3) loadMore();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, index, displayItems.length, hasMore, isFetchingMore]);

  // 依赖里的 source/长度每变一次本来就要新函数，useCallback 省不了重建。
  // 别按「React Compiler 已启用」的口径写注释——本项目没装编译器插件。
  // clamp 用 displayItems:游标在这张表上,pin 活跃时它比 items 长 1
  const step = (dir: 1 | -1) => {
    setIndexes((prev) => {
      const max = Math.max(0, displayItems.length - 1);
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

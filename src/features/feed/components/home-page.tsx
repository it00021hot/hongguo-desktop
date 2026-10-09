import { useEffect, useMemo, useRef, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { TopBarTab, TopBarTabsPortal } from '@/components/layout/top-bar-tabs';
import { PlayerView } from '@/features/player/components/player-page';
import { play } from '@/lib/ipc/commands';
import {
  isRenderableCover,
  useAccount,
  useBookshelf,
  useFeed,
  usePrefetchSeriesEpisodes,
  useSeriesEpisodes,
  useSeriesProgress,
  useWatchHistory,
  useWebCover,
} from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { t } from '@/i18n';

/**
 * 首页：沉浸式播放器流（第三方同款形态）。
 *
 * 视频**铺满整页**，顶部居中是分类 tab 胶囊（推荐 / 漫剧 / 真人）——三者
 * 来自书城 cell 换一换的同一个接口（tab_type 16/36/39，hgplayer 首页同款
 * tab 表），内容随服务端时间桶轮换。滚轮 / ↑↓ 在当前源内切上一部/下一部
 * 剧；剧名/集数/简介叠加在画面左下，鼠标静止 3 秒连同互动栏一起淡出。
 * （hgplayer 的第四个 tab「关注」= 登录后的追更流，由收藏页承载，并且
 * 推荐 tab 头部已种入云端历史+书架，「你的内容」不缺位。）
 *
 * 切换要快：当前剧进入时就预取下一部的分集档案与流（本地缺失自动回落解析），
 * 真正切过去时只剩取流时间。
 */

/** 顶部 tab 的三个源（书城 cell 换一换：推荐 16 / 漫剧 36 / 真人剧 39）。 */
type StreamSource = 'feed' | 'comic' | 'human';

/**
 * 推荐流源 → 书城 cell 换一换的 tab_type（hgplayer bookmall/tab 下发：
 * 推荐 16 / 漫剧 36 / 真人剧 39；关注 45 是登录后的追更流，由收藏页承载）。
 */
const FEED_TAB: Record<StreamSource, string> = {
  feed: '16',
  comic: '36',
  human: '39',
};

/** 源内一条：沉浸流只消费这几个身份字段。 */
interface StreamItem {
  seriesId: string;
  title: string;
  cover: string;
  /** 横版封面（信息流才有）：全屏窗口的占位图用它，竖版 3:4 拉伸铺满横屏会糊。 */
  horizCover: string;
  /** 热度文本（信息流条目带，剧名上方展示；hgplayer 1.1.6 同款） */
  heatText?: string;
  /** 季角标（「第1季」；剧名前 pill） */
  seasonTag?: string;
  /** 官方运营角标（「新剧/爆剧/红果首发」；剧名前标签） */
  badge?: string;
}

const TABS: { id: StreamSource; labelKey: string }[] = [
  { id: 'feed', labelKey: 'home.tabFeed' },
  { id: 'comic', labelKey: 'home.tabComic' },
  { id: 'human', labelKey: 'home.tabHuman' },
];

const SOURCE_KEY = 'hongguo.feedSource';

function readSource(): StreamSource {
  try {
    const v = window.localStorage.getItem(SOURCE_KEY) as StreamSource | null;
    if (v && TABS.some((t) => t.id === v)) return v;
  } catch {
    // 读不到就用默认
  }
  return 'feed';
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

  const [source, setSource] = useState<StreamSource>(readSource);
  /** 每源独立游标：切 tab 回来还在上次刷到的位置。 */
  const [indexes, setIndexes] = useState<Record<StreamSource, number>>({
    feed: 0,
    comic: 0,
    human: 0,
  });

  const feed = useFeed(FEED_TAB[source]);
  const prefetchEpisodes = usePrefetchSeriesEpisodes();
  const setTarget = usePlayerStore((s) => s.setTarget);

  // 登录后的「你的内容」种子：云端观看历史 + 书架（追更）排在推荐流最前
  // ——同一个号打开就是你的剧，对齐第三方（它的首页就是书架/续看驱动）。
  // 历史/书架条目没有横版封面，标题等 currentSeries 解析出来自然显示；
  // 未登录时种子为空，行为与从前一致。
  const { data: account } = useAccount();
  const { data: history, isPending: historyPending } = useWatchHistory();
  const { data: bookshelf } = useBookshelf();
  const seedItems = useMemo<StreamItem[]>(() => {
    if (!account) return [];
    const seen = new Set<string>();
    const out: StreamItem[] = [];
    for (const h of history?.items ?? []) {
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
  }, [account, history, bookshelf]);

  // 三 tab 同一数据面，统一成 StreamItem[]。
  // useMemo 稳定身份：数组每渲染新建会让依赖它的 effect/哨兵反复重跑。
  const feedItems = feed.items;
  // 「你的内容」种子只属于推荐 tab：漫剧/真人是服务端类型 tab，头一条必须
  // 是该类型的剧——种子(最近看过)排头会把刚看完的那部顶在前面，点 tab
  // 看起来毫无反应(实测:刚看完动漫点真人,头部还是那部动漫)。
  const items: StreamItem[] = useMemo(() => {
    const activeSeeds = source === 'feed' ? seedItems : [];
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

  // 游标越界(榜单/新剧源切换)夹到已加载尾部，后台翻页跟上。
  const rawIndex = indexes[source];
  const index = Math.min(rawIndex, Math.max(0, items.length - 1));
  const current = items[index];
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
  // - 云端观看历史：跨客户端（hgplayer / 官方 App 看过的也算），种子区
  //   已在用同一查询，这里共享缓存
  const progressQuery = useSeriesProgress(currentId ?? '');
  const localProgress = progressQuery.data;
  const playingId = usePlayerStore((s) => s.seriesId);
  const setResumeHint = usePlayerStore((s) => s.setResumeHint);

  // 回首页恢复（切菜单回来不换视频）：store 目标剧还在信息流里 → 游标
  // 对回那部剧。信息流游标是本组件 state，切菜单重挂载会归零，不恢复的
  // 话起点决策会把目标换成第一部、正在看的视频被顶掉。
  // 渲染期 setState（官方 adjust 模式）：置位后本组件立即以新游标重渲染，
  // effect 只在最终提交后跑一次——起点决策自然对准恢复的那部剧，无需
  // 额外闩锁。只恢复一次：之后用户滚走再滚回，不再拽游标。
  const [restored, setRestored] = useState(false);
  if (!restored && items.length > 0) {
    setRestored(true);
    if (playingId) {
      const idx = items.findIndex((i) => i.seriesId === playingId);
      if (idx > 0) setIndexes((prev) => ({ ...prev, [source]: idx }));
    }
  }

  // 档案 + 两路进度源就绪 → 定起点。看过快完的（≥95%）自动跳下一集。
  // 云端历史的集内位置塞进 resumeHint：播放器起播在本地没有该集播放
  // 档案（resumeAt=0）时用它 seek——跨客户端续播连进度都对上。
  //
  // 每部剧只在**成为当前剧**时定一次起点：正在看的时候历史刷新/进度
  // 上报不能把目标拽走（store 目标还停在本剧就直接让位）；滚走再滚回
  // 来时会重新对号本地档案，接的是你离开时的那集。
  // 恢复游标的渲染期 setState 让 effect 只在游标生效后的提交上跑一次，
  // 恢复在途（档案未跟上）时 currentSeries 为空自然空转，无需额外闩锁。
  useEffect(() => {
    if (!currentSeries) return;
    if (playingId === currentSeries.seriesId) return;
    if (progressQuery.isPending || historyPending) return;

    const episodes = currentSeries.episodes;
    const hasNext = (n: number) => episodes.some((e) => e.vidIndex === n);
    let idx = 1;
    let positionMs = 0;

    if (localProgress) {
      const ratio =
        localProgress.duration > 0
          ? localProgress.currentTime / localProgress.duration
          : 0;
      if (ratio >= 0.95) {
        idx = hasNext(localProgress.vidIndex + 1) ? localProgress.vidIndex + 1 : 1;
      } else {
        idx = localProgress.vidIndex;
      }
    } else {
      const h = history?.items.find((i) => i.seriesId === currentSeries.seriesId);
      if (h) {
        idx = Math.max(1, h.vidIndex);
        if (h.durationMs > 0 && h.positionMs / h.durationMs >= 0.95) {
          idx = hasNext(idx + 1) ? idx + 1 : 1;
        }
        positionMs = h.positionMs;
      }
    }

    setTarget(currentSeries.seriesId, idx);
    if (positionMs > 3000) {
      setResumeHint({ seriesId: currentSeries.seriesId, vidIndex: idx, positionMs });
    }
  }, [
    currentSeries,
    playingId,
    localProgress,
    progressQuery.isPending,
    history,
    historyPending,
    setTarget,
    setResumeHint,
  ]);

  // 占位封面：横版优先（竖版 3:4 被 object-cover 拉满横屏窗口=整屏发糊）；
  // 再挑 WebView 渲染得了的 URL。eagerProxy：HEIC 源不等 webp 网络请求，
  // 立即给本地转码代理——切剧瞬间的全屏占位黑一秒都是「卡顿感」的来源
  const rawCover = current?.horizCover || current?.cover || '';
  const { data: webCover } = useWebCover(rawCover);
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

  // 预取下一部剧第 1 集的**流**（取流表 + 渐进填充）：滚过去时缓存已就绪，
  // 首帧只等头部数据。延迟 1.5 秒让当前剧先把首帧吃下来，别抢带宽；
  // 同一部剧只取一次。
  const streamPrefetched = useRef(new Set<string>());
  useEffect(() => {
    if (!nextItem) return;
    const id = nextItem.seriesId;
    if (streamPrefetched.current.has(id)) return;
    streamPrefetched.current.add(id);
    const timer = setTimeout(() => {
      void play.prefetch(id).catch(() => {
        // 失败不算数：下次这个剧再成为「下一部」时允许重试
        streamPrefetched.current.delete(id);
      });
    }, 1_500);
    return () => clearTimeout(timer);
  }, [nextItem]);

  // 快滑到源尾部时预取下一页；游标越界时也靠它链式拉齐
  useEffect(() => {
    if (loadMore && hasMore && !isFetchingMore && items.length - index <= 3) loadMore();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, index, items.length, hasMore, isFetchingMore]);

  // 不用手动 useCallback：依赖里的 source/长度每变一次本来就要新函数，
  // React Compiler 能自动 memo；手动写反而和编译器打架（preserve-manual-memoization）。
  const step = (dir: 1 | -1) => {
    setIndexes((prev) => {
      const max = Math.max(0, items.length - 1);
      return { ...prev, [source]: Math.min(Math.max(prev[source] + dir, 0), max) };
    });
  };

  // 分类 tab 现已 portal 进 AppShell 顶栏（见 TopBarTabs），不再悬浮画面。
  // 样式与排行榜/新剧的内容 tab 同一套主题语义色——全应用一个 tab 语言，
  // 自动跟随亮暗主题。**重复点当前 tab = 换一批**（「再刷一组」的最直接
  // 入口）：重开会话取服务端当前时间桶的新一批。
  const pickTab = (id: StreamSource) => {
    if (id === source) {
      feed.restart();
      setIndexes((prev) => ({ ...prev, [id]: 0 }));
      return;
    }
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

  // 切 tab 后新源首批在拉：**不整页换骨架屏**——store 里还播着上一部，
  // 主树继续渲染 PlayerView 顶住画面，顶部细进度条已给「正在切换」反馈。
  // 骨架屏只在冷启动（store 无目标、真的一无所有）才出现。
  if (isLoading && items.length === 0 && !playingId) {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <Skeleton className="min-h-0 flex-1 rounded-xl" />
        <Skeleton className="h-4 w-1/3" />
        <Skeleton className="h-3 w-2/3" />
      </div>
    );
  }
  // 分集档案解析失败（含 Rust 重启后 invoke 挂起转超时）：必须给重试入口，
  // 静默停在「还没有选择剧集」就是用户反复看到的那个神秘界面
  if (seriesQuery.isError && current) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
        <p>{t('feed.loadFailed')}</p>
        <p className="text-destructive text-xs">{String(seriesQuery.error)}</p>
        <Button variant="outline" size="sm" onClick={() => void seriesQuery.refetch()}>
          <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
          {t('feed.retry')}
        </Button>
      </div>
    );
  }
  if (loadError && items.length === 0) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
        <p>{t('feed.loadFailed')}</p>
        <p className="text-destructive text-xs">{loadError}</p>
        <Button variant="outline" size="sm" onClick={retrySource}>
          <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
          {t('feed.retry')}
        </Button>
      </div>
    );
  }
  // 首批在拉时不是空态：画面继续播上一次的剧，拉到位自动过去
  if (!current && items.length === 0 && !isLoading) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-2 py-16">
        <p className="text-sm">{t('home.empty')}</p>
        <Button variant="outline" size="sm" onClick={retrySource}>
          {t('feed.retry')}
        </Button>
      </div>
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
      <TopBarTabs source={source} onPick={pickTab} />
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
        <PlayerView onWheelStep={step} coverUrl={coverForPlayer} overlayMeta={overlayMeta} />
      </div>
    </div>
  );
}

/**
 * 分类 tab 栏（顶栏中部插槽，共享件见 top-bar-tabs.tsx）。
 */
function TopBarTabs({
  source,
  onPick,
}: {
  source: StreamSource;
  onPick: (id: StreamSource) => void;
}) {
  return (
    <TopBarTabsPortal>
      {TABS.map((tab) => (
        <TopBarTab key={tab.id} active={source === tab.id} onClick={() => onPick(tab.id)}>
          {t(tab.labelKey)}
        </TopBarTab>
      ))}
    </TopBarTabsPortal>
  );
}

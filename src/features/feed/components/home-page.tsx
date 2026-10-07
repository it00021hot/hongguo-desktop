import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { TopBarTab, TopBarTabsPortal } from '@/components/layout/top-bar-tabs';
import { PlayerView } from '@/features/player/components/player-page';
import { play } from '@/lib/ipc/commands';
import {
  isRenderableCover,
  useFeed,
  useNewDrama,
  usePrefetchSeriesEpisodes,
  useRank,
  useSeriesEpisodes,
  useWebCover,
} from '@/lib/queries';
import { usePlayerStore } from '@/lib/stores/player';
import { useAccount, useBookshelf, useWatchHistory } from '@/lib/queries';
import { t } from '@/i18n';

/**
 * 首页：沉浸式播放器流（第三方同款形态）。
 *
 * 视频**铺满整页**，顶部居中是分类 tab 胶囊（推荐 / 热播 / 新剧）——
 * 切 tab 就是换一个推荐源，滚轮 / ↑↓ 在当前源内切上一部/下一部剧；
 * 剧名/集数/简介叠加在画面左下，鼠标静止 3 秒连同互动栏一起淡出。
 *
 * 切换要快：当前剧进入时就预取下一部的分集档案与流（本地缺失自动回落解析），
 * 真正切过去时只剩取流时间。
 */

/** 顶部 tab 的五个源：推荐流（全部/漫剧/真人，同一接口按内容类型过滤） / 热播榜 / 新剧。 */
type StreamSource = 'feed' | 'comic' | 'human' | 'hot' | 'new';

/** 推荐流源 → 官方「体裁」过滤键（discover.rs 的 GENRE_*；推荐=不过滤）。 */
const FEED_GENRE: Partial<Record<StreamSource, string>> = {
  comic: 'comic_series',
  human: 'short_play',
};

/** 源内一条：沉浸流只消费这几个身份字段。 */
interface StreamItem {
  seriesId: string;
  title: string;
  cover: string;
  /** 横版封面（信息流才有）：全屏窗口的占位图用它，竖版 3:4 拉伸铺满横屏会糊。 */
  horizCover: string;
  /** 该条目所属流的体裁键（'comic_series'/'short_play'；口味统计计数用） */
  genre?: string;
  /** 热度文本（信息流条目带，剧名上方展示；hgplayer 1.1.6 同款） */
  heatText?: string;
  /** 季角标（「第1季」；剧名前 pill） */
  seasonTag?: string;
  /** 官方运营角标（「新剧/爆剧/红果首发」；剧名前标签） */
  badge?: string;
}

/** 口味统计（localStorage 持久化）：看过哪类多，推荐 tab 就按哪类过滤。
 *  键是官方体裁 id（comic_series/short_play）；旧的数字键一律不认。 */
const TYPE_STATS_KEY = 'hongguo.typeStats';

const KNOWN_GENRES = ['comic_series', 'short_play', 'ai_series'] as const;

type TypeStats = Partial<Record<(typeof KNOWN_GENRES)[number], number>>;

function readTypeStats(): TypeStats {
  try {
    const raw = JSON.parse(window.localStorage.getItem(TYPE_STATS_KEY) ?? '') as Record<
      string,
      unknown
    >;
    const out: TypeStats = {};
    for (const g of KNOWN_GENRES) {
      const v = raw[g];
      if (typeof v === 'number' && v > 0) out[g] = v;
    }
    return out;
  } catch {
    return {};
  }
}

function writeTypeStats(v: TypeStats): void {
  try {
    window.localStorage.setItem(TYPE_STATS_KEY, JSON.stringify(v));
  } catch {
    // 存不进只影响下次启动的口味
  }
}

/** 样本够多且一边倒时给出口味体裁，否则 undefined（不过滤）。 */
function dominantGenre(stats: TypeStats): string | undefined {
  const total = Object.values(stats).reduce((a, b) => a + (b ?? 0), 0);
  if (total < 5) return undefined;
  const sorted = Object.entries(stats).sort((a, b) => (b[1] ?? 0) - (a[1] ?? 0));
  const top = sorted[0];
  if (!top) return undefined;
  return (top[1] ?? 0) / total >= 0.6 ? top[0] : undefined;
}

const TABS: { id: StreamSource; labelKey: string }[] = [
  { id: 'feed', labelKey: 'home.tabFeed' },
  { id: 'comic', labelKey: 'home.tabComic' },
  { id: 'human', labelKey: 'home.tabHuman' },
  { id: 'hot', labelKey: 'home.tabHot' },
  { id: 'new', labelKey: 'home.tabNew' },
];

/**
 * 推荐流的**入口游标**：持久化到 localStorage——「每次打开换新剧」要跨
 * 重启成立，只在内存里记着重启就归零，永远从第 1 部开始。
 *
 * 同一个 offset 服务端返回的是同一批，refetch 换不来新内容；
 * 入口往后挪一页才是确定性的换血。到上限就回绕从头轮换
 * （推荐流本身也会随时间轮换，转一圈回来多半已是新内容）。
 */
const ENTRY_PAGE = 12;
/** 入口上限（批）：防止重启次数多了以后冷启动要串行补一长串页。 */
const ENTRY_CAP = 8;
const ENTRY_KEY = 'hongguo.feedEntry';

type FeedEntryKey = 'feed' | 'comic' | 'human' | 'new';

function readEntry(): Record<FeedEntryKey, number> {
  const zero: Record<FeedEntryKey, number> = { feed: 0, comic: 0, human: 0, new: 0 };
  try {
    const raw = JSON.parse(window.localStorage.getItem(ENTRY_KEY) ?? '') as Partial<
      Record<FeedEntryKey, number>
    >;
    for (const k of Object.keys(zero) as FeedEntryKey[]) {
      if (Number.isFinite(raw[k]) && (raw[k] as number) >= 0) zero[k] = raw[k] as number;
    }
    return zero;
  } catch {
    return zero;
  }
}

function writeEntry(v: Record<FeedEntryKey, number>): void {
  try {
    window.localStorage.setItem(ENTRY_KEY, JSON.stringify(v));
  } catch {
    // 存不进去只影响下次启动的起点，本次会话内仍正常前进
  }
}

const entryCursor = readEntry();
/** 本次应用会话是否已经前进过（会话内反复进出首页不前进，保持续看位置）。 */
let advancedThisSession = false;

/** 入口前进一批（到上限回绕）。返回前进后的值。 */
function advanceEntry(key: FeedEntryKey): number {
  const next = entryCursor[key] + ENTRY_PAGE;
  entryCursor[key] = next >= ENTRY_CAP * ENTRY_PAGE ? 0 : next;
  writeEntry(entryCursor);
  return entryCursor[key];
}

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
  const [source, setSource] = useState<StreamSource>(readSource);
  const pickSource = (id: StreamSource) => {
    setSource(id);
    bumpType(FEED_GENRE[id], 3);
    try {
      window.localStorage.setItem(SOURCE_KEY, id);
    } catch {
      // 存不进只影响下次启动
    }
  };
  /** 每源独立游标：切 tab 回来还在上次刷到的位置（流式源从入口游标起步）。 */
  const [indexes, setIndexes] = useState<Record<StreamSource, number>>({
    feed: 0,
    comic: 0,
    human: 0,
    hot: 0,
    new: entryCursor.new,
  });

  const isFeedSource = source === 'feed' || FEED_GENRE[source] != null;
  /** 口味统计：看过（或主动选过）的类型记权重，推荐 tab 据此自适应过滤。 */
  const [typeStats, setTypeStats] = useState<TypeStats>(readTypeStats);
  /** 各推荐源的**取流起始偏移**(持久化游标):换一批/每次启动直接从
   *  偏移处取新批,而不是在已加载列表里往前走。 */
  const [feedStarts, setFeedStarts] = useState<Record<'feed' | 'comic' | 'human', number>>({
    feed: entryCursor.feed,
    comic: entryCursor.comic,
    human: entryCursor.human,
  });
  const countedRef = useRef<Set<string>>(new Set());
  const bumpType = useCallback((genre: string | undefined, weight = 1) => {
    const g = KNOWN_GENRES.find((x) => x === genre);
    if (!g) return;
    setTypeStats((prev) => ({ ...prev, [g]: (prev[g] ?? 0) + weight }));
  }, []);
  // updater 必须纯：落盘放 effect（hooks 编译器要求）
  useEffect(() => {
    writeTypeStats(typeStats);
  }, [typeStats]);
  // 推荐 tab 的语义 = 跟随口味：样本够多且集中时按主导类型过滤，
  // 否则给官方混合流。漫剧/真人 tab 永远是显式指定的类型。
  const recommendGenre = source === 'feed' ? dominantGenre(typeStats) : undefined;
  const activeFeedStart =
    source === 'feed' || source === 'comic' || source === 'human' ? feedStarts[source] : 0;
  const feed = useFeed(source === 'feed' ? recommendGenre : FEED_GENRE[source], activeFeedStart);
  const hot = useRank('all', 'ranklist_hot_sc', '');
  const fresh = useNewDrama(2);
  const prefetchEpisodes = usePrefetchSeriesEpisodes();
  const setTarget = usePlayerStore((s) => s.setTarget);

  // 登录后的「你的内容」种子：云端观看历史 + 书架（追更）排在推荐流最前
  // ——同一个号打开就是你的剧，对齐第三方（它的首页就是书架/续看驱动）。
  // 历史/书架条目没有横版封面与类型（书架有 content_type），标题等
  // currentSeries 解析出来自然显示；未登录时种子为空，行为与从前一致。
  const { data: account } = useAccount();
  const { data: history } = useWatchHistory();
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
      // 书架的 content_type 数字语义不可靠(实测 1 混着动漫),不参与口味
      out.push({ seriesId: b.seriesId, title: '', cover: '', horizCover: '' });
    }
    return out;
  }, [account, history, bookshelf]);

  // 三源统一成 StreamItem[]（rank/new 本身就是 RankItem，只取身份字段）
  const feedItems = feed.items;
  const hotItems = hot.data?.items ?? [];
  const freshItems = fresh.items;
  // 「你的内容」种子只属于推荐 tab：漫剧/真人是显式类型 tab，头一条必须
  // 是该类型的剧——种子(最近看过)排头会把刚看完的那部顶在前面，点 tab
  // 看起来毫无反应(实测:刚看完动漫点真人,头部还是那部动漫)。
  const activeSeeds = source === 'feed' ? seedItems : [];
  const items: StreamItem[] = isFeedSource
    ? [
        ...activeSeeds,
        ...feedItems
          .filter(
            (i, idx, arr) =>
              // 服务端翻页会重复下发同一批里的条目(实测第二页重复 10/18),
              // 连种子一起按 seriesId 去重
              !activeSeeds.some((seed) => seed.seriesId === i.seriesId) &&
              arr.findIndex((x) => x.seriesId === i.seriesId) === idx,
          )
          .map((i) => ({
            seriesId: i.seriesId,
            title: i.title,
            cover: i.cover,
            horizCover: i.horizCover,
            genre: source === 'feed' ? recommendGenre : FEED_GENRE[source],
            heatText: i.heatText,
            seasonTag: i.seasonTag,
            badge: i.badge,
          })),
      ]
    : source === 'hot'
      ? hotItems.map((i) => ({
          seriesId: i.seriesId,
          title: i.title,
          cover: i.cover,
          horizCover: '',
        }))
      : freshItems.map((i) => ({
          seriesId: i.seriesId,
          title: i.title,
          cover: i.cover,
          horizCover: '',
        }));

  // 尾部翻页能力（热榜是单页，没有更多）
  const hasMore = isFeedSource ? feed.hasMore : source === 'new' ? fresh.hasMore : false;
  const isFetchingMore = isFeedSource
    ? feed.isFetchingMore
    : source === 'new'
      ? fresh.isFetchingMore
      : false;
  const loadMore = isFeedSource ? feed.loadMore : source === 'new' ? fresh.loadMore : null;

  // 入口换血改走「取流偏移」(见 feedStarts),索引不再承担跨批走路;
  // 游标越界(新剧的持久游标)一律夹到已加载尾部,后台翻页跟上。
  const rawIndex = indexes[source];
  const index = Math.min(rawIndex, Math.max(0, items.length - 1));
  const current = items[index];
  const currentId = current?.seriesId;

  // 信息流条目的展示标记（热度/季角标）：档案接口没有这两个字段，
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

  // 档案就位 → 设为播放目标（从第 1 集开始，看过的剧由 resumeAt 接进度）。
  // 顺带做口味记账：信息流条目每部只记一次（切回切出不重复加权）。
  useEffect(() => {
    if (!currentSeries) return;
    setTarget(currentSeries.seriesId, 1);
    if (current && current.genre && !countedRef.current.has(current.seriesId)) {
      countedRef.current.add(current.seriesId);
      bumpType(current.genre);
    }
  }, [currentSeries, setTarget, current, bumpType]);

  // 占位封面：横版优先（竖版 3:4 被 object-cover 拉满横屏窗口=整屏发糊）；
  // 再挑 WebView 渲染得了的 URL。eagerProxy：HEIC 源不等 webp 网络请求，
  // 立即给本地转码代理——切剧瞬间的全屏占位黑一秒都是「卡顿感」的来源
  const rawCover = current?.horizCover || current?.cover || '';
  const { data: webCover } = useWebCover(currentId ?? '', rawCover, true);
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

  // 快滑到源尾部时预取下一页；入口游标越界时也靠它链式拉齐
  useEffect(() => {
    if (loadMore && hasMore && !isFetchingMore && items.length - index <= 3) loadMore();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, index, items.length, hasMore, isFetchingMore]);

  // 「每次打开刷新」的落点：**每次应用启动**进入首页时，把入口预前进一批
  // （本次从上次存的游标起步，下次启动从下一批开始）。会话内反复进出首页
  // 不前进——那是回来看剧，不是重新打开推荐，位置应当续上。
  useEffect(() => {
    if (advancedThisSession) return;
    advancedThisSession = true;
    advanceEntry('feed');
    advanceEntry('new');
  }, []);

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
  // 入口）；热播榜是固定榜单，重点不换。
  const pickTab = (id: StreamSource) => {
    pickSource(id);
    if (id !== source || id === 'hot') return;
    // 换一批:feed 系推进**取流偏移**(新批直达,索引回 0);
    // 新剧源还是索引走路(它单页快,不值得换机制)
    if (id === 'feed' || id === 'comic' || id === 'human') {
      const next = advanceEntry(id);
      setFeedStarts((prev) => ({ ...prev, [id]: next }));
      setIndexes((prev) => ({ ...prev, [id]: 0 }));
    } else if (id === 'new') {
      const next = advanceEntry(id);
      setIndexes((prev) => ({ ...prev, [id]: next }));
    }
  };

  // 漫剧/真人走 feed 钩子，状态必须同源取——按 source 名逐个三元会让
  // 这两个 tab 落到 fresh（新剧）的状态上：首批在拉时 isLoading 恒 false，
  // 空态分支抢跑，表现就是「切类型秒变推荐流暂时为空」（实测复现）。
  const isLoading = isFeedSource
    ? feed.isLoading
    : source === 'hot'
      ? hot.isPending
      : fresh.isLoading;
  const loadError = isFeedSource
    ? feed.error
    : source === 'hot'
      ? hot.error
        ? hot.error instanceof Error
          ? hot.error.message
          : String(hot.error)
        : null
      : fresh.error;
  // 重试当前源：feed 系重拉当前批；热榜重挂榜单；新剧重拉列表
  const retrySource = () => {
    if (isFeedSource) return void feed.refresh();
    if (source === 'hot') return void hot.refetch();
    return void fresh.refresh();
  };
  // 空态的重试对 feed 系必须是「换一批」：同一个 offset 重拉回来还是空，
  // 只有推进入口游标才真的有机会拿到内容
  const retryEmpty = () => {
    if (source === 'feed' || source === 'comic' || source === 'human') {
      const next = advanceEntry(source);
      setFeedStarts((prev) => ({ ...prev, [source]: next }));
      setIndexes((prev) => ({ ...prev, [source]: 0 }));
      return;
    }
    retrySource();
  };

  const playingId = usePlayerStore((s) => s.seriesId);
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
        <Button variant="outline" size="sm" onClick={retryEmpty}>
          {t('feed.retry')}
        </Button>
      </div>
    );
  }

  // 切 tab 后首批在拉（旧画面继续播）：亮顶部细进度条给「正在切换」
  // 的反馈——没有它就是「点了没反应」（用户实测反馈）。
  const sourceBooting =
    (isFeedSource && feed.isLoading) ||
    (source === 'hot' && hot.isPending) ||
    (source === 'new' && fresh.isLoading);

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

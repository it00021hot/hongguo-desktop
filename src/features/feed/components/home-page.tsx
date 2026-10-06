import { useCallback, useEffect, useRef, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
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
import { t } from '@/i18n';
import { cn } from '@/lib/utils';

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

/** 顶部 tab 的三个源：官方推荐流 / 热播榜 / 新剧（全部频道）。 */
type StreamSource = 'feed' | 'hot' | 'new';

/** 源内一条：沉浸流只消费这几个身份字段。 */
interface StreamItem {
  seriesId: string;
  title: string;
  cover: string;
  /** 横版封面（信息流才有）：全屏窗口的占位图用它，竖版 3:4 拉伸铺满横屏会糊。 */
  horizCover: string;
}

const TABS: { id: StreamSource; labelKey: string }[] = [
  { id: 'feed', labelKey: 'home.tabFeed' },
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

function readEntry(): Record<'feed' | 'new', number> {
  try {
    const raw = JSON.parse(window.localStorage.getItem(ENTRY_KEY) ?? '') as Record<
      'feed' | 'new',
      number
    >;
    return {
      feed: Number.isFinite(raw.feed) && raw.feed >= 0 ? raw.feed : 0,
      new: Number.isFinite(raw.new) && raw.new >= 0 ? raw.new : 0,
    };
  } catch {
    return { feed: 0, new: 0 };
  }
}

function writeEntry(v: Record<'feed' | 'new', number>): void {
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
function advanceEntry(key: 'feed' | 'new'): number {
  const next = entryCursor[key] + ENTRY_PAGE;
  entryCursor[key] = next >= ENTRY_CAP * ENTRY_PAGE ? 0 : next;
  writeEntry(entryCursor);
  return entryCursor[key];
}

export function HomePage() {
  const [source, setSource] = useState<StreamSource>('feed');
  /** 每源独立游标：切 tab 回来还在上次刷到的位置（流式源从入口游标起步）。 */
  const [indexes, setIndexes] = useState<Record<StreamSource, number>>({
    feed: entryCursor.feed,
    hot: 0,
    new: entryCursor.new,
  });

  const feed = useFeed();
  const hot = useRank('all', 'ranklist_hot_sc', '');
  const fresh = useNewDrama(2);
  const prefetchEpisodes = usePrefetchSeriesEpisodes();
  const setTarget = usePlayerStore((s) => s.setTarget);

  // 三源统一成 StreamItem[]（rank/new 本身就是 RankItem，只取身份字段）
  const feedItems = feed.items;
  const hotItems = hot.data?.items ?? [];
  const freshItems = fresh.items;
  const items: StreamItem[] =
    source === 'feed'
      ? feedItems
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
  const hasMore = source === 'feed' ? feed.hasMore : source === 'new' ? fresh.hasMore : false;
  const isFetchingMore =
    source === 'feed' ? feed.isFetchingMore : source === 'new' ? fresh.isFetchingMore : false;
  const loadMore = source === 'feed' ? feed.loadMore : source === 'new' ? fresh.loadMore : null;

  // 入口游标可能越界（下一批还在拉）：pending 期间不切换播放目标——
  // 画面继续播上一次的剧，等这一批拉到位再过去，不闪播流尾部。
  const rawIndex = indexes[source];
  const pending = source !== 'hot' && hasMore && items.length <= rawIndex;
  const index = pending ? rawIndex : Math.min(rawIndex, Math.max(0, items.length - 1));
  const current = pending ? undefined : items[index];
  const currentId = current?.seriesId;

  /**
   * 当前剧的档案：走 `get_series_episodes`（本地命中秒回，缺失才回落解析），
   * **不走 resolve_series**——那是每次都打网络的解析，预取的缓存它吃不到，
   * 滚轮切剧会在「正在准备」上白等一拍。预取 effect 已经把下一部剧的
   * 分集档案灌进同一份缓存，这里直接命中。
   */
  const { data: currentSeries } = useSeriesEpisodes(currentId ?? '');

  // 档案就位 → 设为播放目标（从第 1 集开始，看过的剧由 resumeAt 接进度）
  useEffect(() => {
    if (!currentSeries) return;
    setTarget(currentSeries.seriesId, 1);
  }, [currentSeries, setTarget]);

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

  const step = useCallback(
    (dir: 1 | -1) => {
      setIndexes((prev) => {
        const max = Math.max(0, items.length - 1);
        return { ...prev, [source]: Math.min(Math.max(prev[source] + dir, 0), max) };
      });
    },
    [items.length, source],
  );

  // 顶部 tab 胶囊条：渲染进播放器的 topChrome 插槽（跟随悬浮层淡出）。
  // 形态对齐第三方（hgplayer）：细边框半透明条 + 选中项淡红底红字——
  // 整条压暗、字重轻，沉浸观看时存在感最低。**重复点当前 tab = 换一批**
  // （「再刷一组」的最直接入口）；热播榜是固定榜单，重点不换。
  const pickTab = (id: StreamSource) => {
    setSource(id);
    if (id === source && id !== 'hot') {
      const next = advanceEntry(id);
      setIndexes((prev) => ({ ...prev, [id]: next }));
    }
  };
  const topChrome = (
    <div className="flex items-center gap-0.5 rounded-lg border border-white/15 bg-black/35 p-0.5">
      {TABS.map((tab) => (
        <button
          key={tab.id}
          type="button"
          onClick={() => pickTab(tab.id)}
          className={cn(
            'cursor-pointer rounded-md px-3.5 py-1 text-[13px] transition-colors',
            source === tab.id
              ? 'bg-red-500/15 font-medium text-red-400'
              : 'text-white/75 hover:text-white',
          )}
        >
          {t(tab.labelKey)}
        </button>
      ))}
    </div>
  );

  const isLoading =
    source === 'feed' ? feed.isLoading : source === 'hot' ? hot.isPending : fresh.isLoading;
  const loadError =
    source === 'feed'
      ? feed.error
      : source === 'hot'
        ? hot.error
          ? hot.error instanceof Error
            ? hot.error.message
            : String(hot.error)
          : null
        : fresh.error;

  if (isLoading && items.length === 0) {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <Skeleton className="min-h-0 flex-1 rounded-xl" />
        <Skeleton className="h-4 w-1/3" />
        <Skeleton className="h-3 w-2/3" />
      </div>
    );
  }
  if (loadError && items.length === 0) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-3 py-16">
        <p>{t('feed.loadFailed')}</p>
        <p className="text-destructive text-xs">{loadError}</p>
        <Button
          variant="outline"
          size="sm"
          onClick={() => (source === 'feed' ? void feed.refresh() : undefined)}
        >
          <Loader2 className="mr-1 size-4 animate-spin" aria-hidden />
          {t('feed.retry')}
        </Button>
      </div>
    );
  }
  // pending（下一批在拉）不是空态：画面继续播上一次的剧，拉到位自动过去
  if (!current && !pending) {
    return (
      <div className="text-muted-foreground flex flex-col items-center gap-2 py-16">
        <p className="text-sm">{t('home.empty')}</p>
        <Button variant="outline" size="sm" onClick={() => void feed.refresh()}>
          {t('feed.retry')}
        </Button>
      </div>
    );
  }

  return (
    // overflow-hidden + 绝对定位铺满：AppShell 的 main 是 overflow-y-auto
    // （别的页面靠它滚），沉浸流内部任何 1px 超高都会冒出一条页面滚动条，
    // 还会把滚轮切剧吃掉——这里整个锁死在视口内。
    <div className="relative h-full min-h-0 overflow-hidden">
      <div className="absolute inset-0">
        {/* 播放器铺满整页；滚轮/↑↓ 在当前 tab 源内切上一部/下一部剧；
            封面给播放器做占位——切剧的取流间隙显示下一部剧的封面而非黑屏 */}
        <PlayerView onWheelStep={step} coverUrl={coverForPlayer} topChrome={topChrome} />
      </div>
    </div>
  );
}

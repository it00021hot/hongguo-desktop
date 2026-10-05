import { useCallback, useMemo, useState } from 'react';
import {
  keepPreviousData,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query';
import {
  browse,
  discover,
  download,
  login,
  merge,
  play,
  rank,
  search,
  series,
  seriesSearch,
  settings,
  storage,
  transcode,
  danmaku as danmakuCmd,
  interact as interactCmd,
  watchHistory,
} from './ipc/commands';
import { useEvent } from './ipc/events';
import { EVENTS } from './ipc/types';
import type {
  Danmaku,
  DownloadProgress,
  DownloadTask,
  FeedItem,
  FeedPage,
  MergeMode,
  MergeTask,
  QueueStatus,
  RankPage,
  SearchPage,
  SearchResult,
} from './schema';

/**
 * TanStack Query 的 key 工厂。
 *
 * 所有 key 必须从这里出，不要在组件里手写字符串——否则失效（invalidate）
 * 时容易漏掉某个 key，导致界面不刷新。
 */
const keys = {
  settings: ['settings'] as const,
  feed: ['feed'] as const,
  newDrama: (gender: number) => ['new-drama', gender] as const,
  seriesList: ['series-list'] as const,
  seriesEpisodes: (id: string) => ['series-episodes', id] as const,
  seriesExtras: (id: string) => ['series-extras', id] as const,
  watchHistory: ['watch-history'] as const,  tasks: ['download-tasks'] as const,
  queueStatus: ['queue-status'] as const,
  mergeTasks: ['merge-tasks'] as const,
  mergeCandidates: ['merge-candidates'] as const,
  mergePreflight: (id: string) => ['merge-preflight', id] as const,
  storageUsage: ['storage-usage'] as const,
  capability: ['decode-capability'] as const,
  browseCategories: ['browse-categories'] as const,
  browseList: (cat: string, genre: string, page: number) =>
    ['browse-list', cat, genre, page] as const,
  seriesSearch: (keyword: string) => ['series-search', keyword] as const,
  danmaku: (vid: string) => ['danmaku', vid] as const,
  interactState: ['interact-state'] as const,
  webCover: (seriesId: string) => ['web-cover', seriesId] as const,
  rank: (selected: string, sub: string, panel: string) =>
    ['rank', selected, sub, panel] as const,
  newCalendar: (date: string) => ['new-calendar', date] as const,
  reservations: (isOnline: boolean) => ['reservations', isOnline] as const,
  account: ['account'] as const,
  appSeriesSearch: (query: string) => ['app-series-search', query] as const,
} satisfies Record<string, unknown>;

// ---------------------------------------------------------------- 设置

export function useSettings() {
  return useQuery({ queryKey: keys.settings, queryFn: settings.get });
}

export function useSaveSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: settings.save,
    onSuccess: (saved) => {
      qc.setQueryData(keys.settings, saved);
      void qc.invalidateQueries({ queryKey: keys.queueStatus });
    },
  });
}

export function useTestProxy() {
  return useMutation({ mutationFn: settings.testProxy });
}

// ---------------------------------------------------------------- 剧集

export function useSeriesList() {
  return useQuery({ queryKey: keys.seriesList, queryFn: series.list });
}

export function useSeriesEpisodes(seriesId: string | null) {
  return useQuery({
    queryKey: keys.seriesEpisodes(seriesId ?? ''),
    queryFn: () => series.episodes(seriesId!),
    enabled: seriesId !== null,
  });
}

export function useSeriesExtras(seriesId: string) {
  return useQuery({
    queryKey: keys.seriesExtras(seriesId),
    queryFn: () => series.extras(seriesId),
    enabled: seriesId !== '',
    staleTime: 10 * 60_000,
  });
}

// ---------------------------------------------------------------- 发现（推荐信息流）

/** 无限滚动查询的最小结构面（只取页面消费的字段，避免深泛型签名）。 */
interface InfiniteStream<TPage> {
  data: { pages: TPage[] } | undefined;
  error: Error | null;
  hasNextPage: boolean;
  isPending: boolean;
  isFetching: boolean;
  isFetchingNextPage: boolean;
  fetchNextPage: () => Promise<unknown>;
  refetch: () => Promise<unknown>;
}

/**
 * 无限滚动流的通用出口：把 useInfiniteQuery 的结果包装成旧手动累积器
 * 的形状（items/hasMore/isLoading/isFetchingMore/error/loadMore/refresh），
 * 页面侧无感迁移。翻页失败不炸整页——旧内容还在，错误就地展示。
 */
function useInfiniteStream<TPage, TItem>(
  query: InfiniteStream<TPage>,
  flatten: (pages: TPage[]) => TItem[],
) {
  const [loadError, setLoadError] = useState<string | null>(null);
  const pages = query.data?.pages;
  const items = useMemo(() => flatten(pages ?? []), [pages, flatten]);

  const loadMore = useCallback(() => {
    if (!query.hasNextPage || query.isFetchingNextPage) return;
    setLoadError(null);
    query.fetchNextPage().catch((e: unknown) => {
      setLoadError(e instanceof Error ? e.message : String(e));
    });
  }, [query]);

  const refresh = useCallback(() => {
    setLoadError(null);
    return query.refetch();
  }, [query]);

  return {
    items,
    hasMore: query.hasNextPage,
    isLoading: query.isPending,
    isRefreshing: query.isFetching && !query.isPending,
    isFetchingMore: query.isFetchingNextPage,
    error: query.error
      ? query.error instanceof Error
        ? query.error.message
        : String(query.error)
      : loadError,
    loadMore,
    refresh,
  };
}

/** pages → 按拉取顺序、seriesId 去重的条目（推荐位轮换会跨页重复）。 */
function feedItems(pages: FeedPage[]): FeedItem[] {
  const seen = new Set<string>();
  const items: FeedItem[] = [];
  for (const page of pages) {
    for (const item of page.items) {
      if (seen.has(item.seriesId)) continue;
      seen.add(item.seriesId);
      items.push(item);
    }
  }
  return items;
}

/**
 * 推荐信息流（无限滚动）。
 *
 * pages 存在 Query 缓存里：切到其它路由再回来秒出已拉内容，不闪骨架屏；
 * staleTime 内完全不重打，超时只后台刷新（配合 RefreshShade 无感过渡）。
 */
export function useFeed() {
  const query = useInfiniteQuery({
    queryKey: keys.feed,
    queryFn: ({ pageParam }) => discover.feed(pageParam),
    initialPageParam: 0,
    getNextPageParam: (last) => (last.hasMore ? last.nextOffset : undefined),
    staleTime: 5 * 60_000,
  });
  return useInfiniteStream(query, feedItems);
}

// ---------------------------------------------------------------- 弹幕

/** 一集的弹幕（按 vid 缓存；后端已按时间轴排序并拉全窗口）。 */
export function useDanmaku(vid: string) {
  const groupId = vid.split(':')[0] ?? '';
  const bookId = vid.split(':')[1] ?? '';
  return useQuery({
    queryKey: keys.danmaku(vid),
    queryFn: () => danmakuCmd.list(groupId, bookId),
    enabled: vid.includes(':'),
    staleTime: 10 * 60_000,
  });
}

// ---------------------------------------------------------------- 互动（点赞 / 收藏 / 发弹幕）

/** 最近互动状态（登录后才拉；匿名接口静默拒）。 */
export function useInteractionState() {
  const { data: account } = useAccount();
  return useQuery({
    queryKey: keys.interactState,
    queryFn: interactCmd.state,
    enabled: !!account,
    staleTime: 60_000,
  });
}

/**
 * 发弹幕：成功后把新弹幕乐观追加进该集的弹幕缓存（服务端列表有延迟，
 * 不追的话用户发完看不见自己的弹幕）。
 */
export function useSendDanmaku() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { vid: string; text: string; offsetMs: number }) => {
      const [groupId, bookId] = input.vid.split(':');
      return interactCmd.sendDanmaku(groupId ?? '', bookId ?? '', input.text, input.offsetMs);
    },
    onSuccess: (commentId, input) => {
      queryClient.setQueryData<Danmaku[]>(keys.danmaku(input.vid), (prev) => {
        const next = prev ?? [];
        return [
          ...next,
          { commentId, text: input.text, offsetMs: input.offsetMs, diggCount: 0 },
        ].sort((a, b) => a.offsetMs - b.offsetMs);
      });
    },
  });
}

/** 点赞 / 取消点赞一集。 */
export function useVideoDigg() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { vid: string; seriesId: string; digg: boolean }) =>
      interactCmd.videoDigg(input.vid, input.seriesId, input.digg),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.interactState }),
  });
}

/** 收藏 / 取消收藏一部剧。 */
export function useSeriesCollect() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { seriesId: string; collect: boolean }) =>
      interactCmd.seriesCollect(input.seriesId, input.collect),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.interactState }),
  });
}

/** 预约 / 取消预约一部剧（复用 2026-10-04 抓包的 uncover_subscribe 端点）。 */
export function useReserveSeries() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { seriesId: string; reserve: boolean }) =>
      rank.reserve(input.seriesId, input.reserve),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: RESERVATIONS_KEY_ROOT }),
  });
}

// ---------------------------------------------------------------- 封面增强

/** HEIC 封面的本地转码代理地址（hongguo-cover 协议的 http 形式，后端 ffmpeg 转 JPEG）。 */
function coverProxyUrl(remote: string): string {
  const bytes = new TextEncoder().encode(remote);
  let bin = '';
  for (const b of bytes) bin += String.fromCharCode(b);
  const b64 = btoa(bin).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
  return `http://hongguo-cover.localhost/c/${b64}`;
}

/**
 * webp 封面懒加载：HEIC 源只在 HEVC 扩展齐全的机器上能直接渲染，先换成
 * 官网版 webp；换不到的（未上线剧官网没有页面）落到 hongguo-cover 本地
 * 转码代理（ffmpeg 下载 HEIC 转 JPEG，磁盘缓存）。两种途径都失败时
 * `data` 为空，组件自己的 onError 占位图兜底。
 */
export function useWebCover(seriesId: string, sourceCover: string) {
  const needs = !isRenderableCover(sourceCover);
  const q = useQuery({
    queryKey: keys.webCover(seriesId),
    queryFn: () => discover.webCover(seriesId),
    enabled: needs && seriesId !== '',
    staleTime: Infinity,
    gcTime: 30 * 60_000,
    retry: false,
  });
  return {
    data: needs
      ? (q.data ?? (q.isSuccess || q.isError ? coverProxyUrl(sourceCover) : undefined))
      : undefined,
  };
}

/** WebView2 能直接渲染的封面格式（与后端 is_renderable_cover 同口径）。 */
export function isRenderableCover(url: string): boolean {
  const path = url.split('?')[0] ?? url;
  const lower = path.toLowerCase();
  return (
    lower.endsWith('.webp') ||
    lower.endsWith('.png') ||
    lower.endsWith('.jpg') ||
    lower.endsWith('.jpeg')
  );
}

// ---------------------------------------------------------------- 排行榜 / 新剧

/**
 * 一个榜单（内容tab × 子榜 × 筛选 组合缓存；榜单一天更新几次，10 分钟内
 * 不重打。切筛选时用 keepPreviousData 保住旧列表，避免整页闪 loading）。
 */
export function useRank(selected: string, sub: string, panel: string) {
  return useQuery({
    queryKey: keys.rank(selected, sub, panel),
    queryFn: () => rank.list(selected, sub, panel),
    staleTime: 10 * 60_000,
    placeholderData: keepPreviousData,
  });
}

/** pages → 顺序条目（新剧推荐无推荐位轮换，直接平铺）。 */
function newDramaItems(pages: RankPage[]) {
  return pages.flatMap((p) => p.items);
}

/**
 * 新剧推荐（无限滚动，按 gender 分缓存）。
 *
 * 每页固定 18 条，下一页 offset 按已拉条数累计；响应不带 has_more，
 * 以空页为终点（旧实现会向空页无限续拉，这里顺手修正）。频道各存一份
 * 缓存，切回看过的频道秒出。
 */
export function useNewDrama(gender: number) {
  const query = useInfiniteQuery({
    queryKey: keys.newDrama(gender),
    queryFn: ({ pageParam }) => rank.newDrama(gender, pageParam),
    initialPageParam: 0,
    getNextPageParam: (last, allPages) =>
      last.items.length > 0 ? allPages.reduce((n, p) => n + p.items.length, 0) : undefined,
    staleTime: 10 * 60_000,
  });
  return useInfiniteStream(query, newDramaItems);
}

/** 上新日历（date 为空串取默认日；切日期保旧列表平滑过渡）。 */
export function useNewCalendar(date: string) {
  return useQuery({
    queryKey: keys.newCalendar(date),
    queryFn: () => rank.calendar(date === '' ? undefined : date),
    staleTime: 10 * 60_000,
    placeholderData: keepPreviousData,
  });
}

/** 我的预约（isOnline：已上线 / 待上线 tab）。 */
export const RESERVATIONS_KEY_ROOT = ['reservations'] as const;
export function useReservations(isOnline: boolean) {
  return useQuery({
    queryKey: keys.reservations(isOnline),
    queryFn: () => rank.reservations(isOnline),
    staleTime: 60_000,
  });
}

/** 当前登录态（null = 未登录）；登录/退出后要主动失效。 */
export function useAccount() {
  return useQuery({
    queryKey: keys.account,
    queryFn: login.status,
    staleTime: 30_000,
  });
}

// ---------------------------------------------------------------- 浏览与搜索

export function useBrowseCategories() {
  return useQuery({
    queryKey: keys.browseCategories,
    queryFn: browse.categories,
    staleTime: Infinity,
  });
}

export function useBrowseList(category: string, genre: string, page: number) {
  return useQuery({
    queryKey: keys.browseList(category, genre, page),
    queryFn: () => browse.list(category, genre, page),
    // 翻页时保留上一页数据，避免白屏
    placeholderData: (prev) => prev,
    staleTime: 30_000,
  });
}

export function useSearch(keyword: string) {
  const kw = keyword.trim();
  return useQuery({
    queryKey: keys.seriesSearch(kw),
    queryFn: () => search.run(kw),
    // 关键词为空就是浏览模式，不该发嗅探请求
    enabled: kw !== '',
    staleTime: 5 * 60_000,
  });
}

export function useResolveSeries() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: string) => series.resolve(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: keys.seriesList });
    },
  });
}

/** 从剧集列表移除一部剧。磁盘清理页的「移除记录」用它。 */
export function useRemoveSeries() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (seriesId: string) => series.remove(seriesId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: keys.seriesList });
    },
  });
}

/** 一次性移除列表里的全部剧集，返回移除条数。 */
export function useRemoveAllSeries() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => series.removeAll(),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: keys.seriesList });
    },
  });
}

// ---------------------------------------------------------------- 下载

/**
 * 下载任务列表。
 *
 * 进度事件每 500ms 一次，失效查询等于整表重拉一遍。这里就地合并进缓存：
 * 任务集合变化仍然由 `useDownloadEvents` 触发重新请求，
 * 纯进度更新只改对应任务的那两个字段，缓存里也只有一份真相。
 */
export function useDownloadTasks() {
  const qc = useQueryClient();

  const applyProgress = useCallback(
    (p: DownloadProgress) => {
      qc.setQueryData<DownloadTask[]>(keys.tasks, (tasks) =>
        tasks?.map((task) =>
          task.id === p.id ? { ...task, downloaded: p.downloaded, total: p.total } : task,
        ),
      );
    },
    [qc],
  );
  useEvent<DownloadProgress>(EVENTS.downloadProgress, applyProgress);

  return useQuery({ queryKey: keys.tasks, queryFn: download.tasks });
}

export function useQueueStatus() {
  return useQuery({ queryKey: keys.queueStatus, queryFn: download.status });
}

export function useDownloadActions() {
  const qc = useQueryClient();
  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: keys.tasks });
    void qc.invalidateQueries({ queryKey: keys.queueStatus });
  };

  return {
    start: useMutation({
      mutationFn: ({ seriesId, vids }: { seriesId: string; vids: number[] }) =>
        download.start(seriesId, vids),
      onSuccess: invalidate,
    }),
    stop: useMutation({ mutationFn: download.stop, onSuccess: invalidate }),
    retry: useMutation({ mutationFn: download.retry, onSuccess: invalidate }),
    retryMany: useMutation({ mutationFn: download.retryMany, onSuccess: invalidate }),
    remove: useMutation({
      mutationFn: ({ ids, withFiles }: { ids: string[]; withFiles: boolean }) =>
        download.remove(ids, withFiles),
      onSuccess: invalidate,
    }),
    pauseAll: useMutation({ mutationFn: download.pauseAll, onSuccess: invalidate }),
    resumeAll: useMutation({ mutationFn: download.resumeAll, onSuccess: invalidate }),
    rescan: useMutation({ mutationFn: download.rescan, onSuccess: invalidate }),
  };
}

/**
 * 订阅下载任务的结构性事件。
 *
 * 进度事件不在这里：它由 `useDownloadTasks` 自己去合并，
 * 在根布局再订一份等于同一事件被处理两遍。
 *
 * 注意：事件名数量固定，所以这里逐个显式调用 `useEvent`，
 * 不能放进循环——React Hooks 规则禁止在条件或循环里调 Hook。
 */
export function useDownloadEvents() {
  const qc = useQueryClient();
  const invalidate = useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.tasks });
    void qc.invalidateQueries({ queryKey: keys.queueStatus });
  }, [qc]);

  useEvent<DownloadTask | null>(EVENTS.downloadTaskAdded, invalidate);
  useEvent<DownloadTask | null>(EVENTS.downloadCompleted, invalidate);
  useEvent<DownloadTask | null>(EVENTS.downloadFailed, invalidate);
  useEvent<DownloadTask | null>(EVENTS.downloadStopped, invalidate);
  useEvent<QueueStatus | null>(EVENTS.downloadQueueChanged, invalidate);
}

// ---------------------------------------------------------------- 合并

export function useMergeTasks() {
  return useQuery({ queryKey: keys.mergeTasks, queryFn: merge.tasks });
}

/**
 * 可合并的剧列表。
 *
 * 候选是按下载队列算出来的，所以下载一完成就要重取：刚下完的那部剧
 * 在这一刻才第一次成为可合并项。挂在下载收尾事件上而不是只靠进页面时拉一次，
 * 否则用户下完切到合并页看到的还是「没有已下载的分集」。
 */
export function useMergeCandidates() {
  const qc = useQueryClient();
  const invalidate = useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.mergeCandidates });
  }, [qc]);

  useEvent(EVENTS.downloadTaskAdded, invalidate);
  useEvent(EVENTS.downloadCompleted, invalidate);
  useEvent(EVENTS.downloadStopped, invalidate);
  useEvent(EVENTS.downloadQueueChanged, invalidate);

  return useQuery({ queryKey: keys.mergeCandidates, queryFn: merge.candidates });
}

export function useMergePreflight(seriesId: string | null) {
  return useQuery({
    queryKey: keys.mergePreflight(seriesId ?? ''),
    queryFn: () => merge.preflight(seriesId!),
    enabled: seriesId !== null,
  });
}

export function useMergeActions() {
  const qc = useQueryClient();
  const invalidate = () => void qc.invalidateQueries({ queryKey: keys.mergeTasks });

  return {
    start: useMutation({
      mutationFn: ({
        seriesId,
        outputName,
        mode,
      }: {
        seriesId: string;
        outputName: string;
        mode: MergeMode;
      }) => merge.start(seriesId, outputName, mode),
      onSuccess: invalidate,
    }),
    remove: useMutation({ mutationFn: merge.remove, onSuccess: invalidate }),
    // 打开产物所在文件夹：不改动任何数据，无需失效查询
    openOutput: useMutation({ mutationFn: merge.openOutput }),
  };
}

/**
 * 订阅合并事件：进度、开始、完成、失败。
 *
 * 兼容合并要逐集转码，可能跑好几分钟，光靠 `useMergeTasks` 的一次性查询
 * 看不到过程。收尾事件触发失效查询，让列表拿到落盘后的最终状态。
 */
export function useMergeEvents() {
  const qc = useQueryClient();
  const invalidate = useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.mergeTasks });
  }, [qc]);

  // 进度**不失效查询**，照下载那边的做法就地合并进缓存（见 useDownloadTasks）。
  // 后端每完成一集才发一次 `merge-progress`，载荷里的 percent 是算好的；而
  // store 里的 percent 恒为 0——合并中途没人写回 store。所以收到事件就重查，
  // 查回来的还是 0，界面就永远停在「合并中…」，跟没订阅一样。
  const applyProgress = useCallback(
    (snapshot: MergeTask) => {
      qc.setQueryData<MergeTask[]>(keys.mergeTasks, (tasks) =>
        tasks?.map((task) =>
          task.id === snapshot.id ? { ...task, percent: snapshot.percent } : task,
        ),
      );
    },
    [qc],
  );
  useEvent<MergeTask>(EVENTS.mergeProgress, applyProgress);
  useEvent(EVENTS.mergeTaskAdded, invalidate);
  useEvent(EVENTS.mergeCompleted, invalidate);
  useEvent(EVENTS.mergeFailed, invalidate);
}

// ---------------------------------------------------------------- 播放与存储

/**
 * 起播。
 *
 * preferOnline 恒为 true：已下载的那一集后端仍优先走本地文件，
 * 没下载的走在线流——与原版一致，否则「点开没下过的集」永远播不了。
 *
 * `definition` 不传则取平台给的最高档；传了但平台没有这一档时后端自动回退，
 * 响应里的 `definition` 是**实际生效**的那档，菜单以它为准。
 */
export function usePlay() {
  return useMutation({
    mutationFn: ({
      seriesId,
      vidIndex,
      definition,
    }: {
      seriesId: string;
      vidIndex: number;
      definition?: number;
    }) => play.series(seriesId, vidIndex, '', true, definition),
  });
}

export function useSavePosition() {
  return useMutation({
    mutationFn: ({
      seriesId,
      vidIndex,
      currentTime,
      duration,
    }: {
      seriesId: string;
      vidIndex: number;
      currentTime: number;
      duration: number;
    }) => play.savePosition(seriesId, vidIndex, currentTime, duration),
  });
}

export function useStorageUsage() {
  return useQuery({ queryKey: keys.storageUsage, queryFn: storage.usage });
}

export function useDecodeCapability() {
  return useQuery({
    queryKey: keys.capability,
    queryFn: transcode.capability,
    // 能力要跑 ffmpeg 试编一帧才能定，不随窗口聚焦重取
    staleTime: Infinity,
  });
}

/**
 * 重新探测 ffmpeg。
 *
 * 装完 ffmpeg 之后**当前进程的环境变量不会更新**，不重探就一直报「未检测到」。
 * 这条命令丢弃后端缓存重新探一次，比让用户去重启应用合理。
 */
export function useRedetectCapability() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: transcode.redetect,
    onSuccess: (data) => {
      qc.setQueryData(keys.capability, data);
    },
  });
}

/**
 * 播放兼容兜底：把解不出来的一集转成 H.264。
 *
 * 只有在确认「有声音没画面」之后才调——提前转码等于白转。
 */
export function useCompatPlayback() {
  return useMutation({ mutationFn: transcode.transcodeForPlayback });
}

export function useStorageActions() {
  const qc = useQueryClient();
  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: keys.storageUsage });
    void qc.invalidateQueries({ queryKey: keys.tasks });
  };
  return {
    deleteSeries: useMutation({ mutationFn: storage.deleteSeries, onSuccess: invalidate }),
    deleteEpisode: useMutation({
      mutationFn: ({ seriesId, vidIndex }: { seriesId: string; vidIndex: number }) =>
        storage.deleteEpisode(seriesId, vidIndex),
      onSuccess: invalidate,
    }),
    deleteAll: useMutation({ mutationFn: storage.deleteAll, onSuccess: invalidate }),
  };
}

// ---------------------------------------------------------------- 云端观看历史

/** 云端观看历史（官方 App「历史」同源；登录后可用，匿名回空表）。 */
export function useWatchHistory() {
  return useQuery({
    queryKey: keys.watchHistory,
    queryFn: () => watchHistory.list(),
    staleTime: 30_000,
  });
}

// ---------------------------------------------------------------- 官方 App 搜索

/** 综合首页的精选与翻页列表有重复：按 seriesId 去重。 */
function appSearchItems(pages: SearchPage[]): SearchResult[] {
  const seen = new Set<string>();
  const items: SearchResult[] = [];
  for (const page of pages) {
    for (const item of page.items) {
      if (seen.has(item.seriesId)) continue;
      seen.add(item.seriesId);
      items.push(item);
    }
  }
  return items;
}

/**
 * 官方 App 搜索（无限滚动）。
 *
 * 首页只有「精选」少数几条（平台搜索的固定形态），`hasMore` 翻页才是
 * 完整列表；翻页必须带首页发放的 searchId。结果按关键词进缓存：
 * 重复搜索同一关键词秒出，不再全量重拉。
 */
export function useSeriesSearchApp(query: string) {
  const kw = query.trim();
  const query_ = useInfiniteQuery({
    queryKey: keys.appSeriesSearch(kw),
    queryFn: ({ pageParam }) =>
      pageParam.offset === 0
        ? seriesSearch.run(kw)
        : seriesSearch.run(kw, pageParam.offset, pageParam.searchId),
    initialPageParam: { offset: 0, searchId: '' },
    getNextPageParam: (last) =>
      last.hasMore ? { offset: last.nextOffset, searchId: last.searchId } : undefined,
    // 空关键词是浏览模式，不该发搜索请求
    enabled: kw !== '',
    staleTime: 5 * 60_000,
  });
  return useInfiniteStream(query_, appSearchItems);
}

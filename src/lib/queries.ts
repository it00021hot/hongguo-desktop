import { useCallback, useMemo, useState } from 'react';
import {
  keepPreviousData,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
  type InfiniteData,
} from '@tanstack/react-query';
import {
  discover,
  download,
  login,
  merge,
  play,
  rank,
  series,
  seriesSearch,
  settings,
  storage,
  transcode,
  danmaku as danmakuCmd,
  interact as interactCmd,
  watchHistory,
} from '@/service/commands';
import { useEvent } from '@/service/tauri/events';
import { isWindows } from './platform';
import { EVENTS } from '@/service/tauri/types';
import type {
  BrowseFilters,
  CommentPage,
  Danmaku,
  DownloadProgress,
  DownloadTask,
  FeedItem,
  FeedPage,
  MergeMode,
  MergeTask,
  QueueStatus,
  RankItem,
  RankPage,
  SearchPage,
  SearchResult,
  InteractionItem,
  InteractionState,
} from '@/service/schema';

/**
 * TanStack Query 的 key 工厂。
 *
 * 所有 key 必须从这里出，不要在组件里手写字符串——否则失效（invalidate）
 * 时容易漏掉某个 key，导致界面不刷新。
 */
export const keys = {
  settings: ['settings'] as const,
  feed: (tab: string) => ['feed', tab] as const,
  newDrama: (gender: number) => ['new-drama', gender] as const,
  seriesEpisodes: (id: string) => ['series-episodes', id] as const,
  seriesProgress: (id: string) => ['series-progress', id] as const,
  watchHistory: ['watch-history'] as const,
  tasks: ['download-tasks'] as const,
  queueStatus: ['queue-status'] as const,
  mergeTasks: ['merge-tasks'] as const,
  mergeCandidates: ['merge-candidates'] as const,
  mergePreflight: (id: string) => ['merge-preflight', id] as const,
  storageUsage: ['storage-usage'] as const,
  storageSeries: ['storage-series'] as const,
  capability: ['decode-capability'] as const,
  browsePanel: ['browse-panel'] as const,
  browseFeed: (filters: BrowseFilters) => ['browse-feed', filters] as const,
  relatedSeries: (seriesId: string) => ['related-series', seriesId] as const,
  seriesMeta: (seriesId: string) => ['series-meta', seriesId] as const,
  seriesComments: (seriesId: string) => ['series-comments', seriesId] as const,
  danmaku: (vid: string) => ['danmaku', vid] as const,
  comments: (vid: string) => ['comments', vid] as const,
  interactState: ['interact-state'] as const,
  bookshelf: ['bookshelf'] as const,
  rank: (selected: string, sub: string, panel: string) => ['rank', selected, sub, panel] as const,
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

export function useSeriesEpisodes(seriesId: string | null) {
  const queryClient = useQueryClient();
  // 旧格式档案在 Rust 侧后台补计数，补完发事件——这里失效自己的缓存，
  // 让计数无感浮现（档案本身早已秒回，不等这次刷新）
  useEvent<string>(EVENTS.seriesArchiveUpdated, (id) => {
    if (id && id === seriesId) {
      void queryClient.invalidateQueries({ queryKey: keys.seriesEpisodes(id) });
    }
  });
  return useQuery({
    queryKey: keys.seriesEpisodes(seriesId ?? ''),
    // 挂起兜底：Rust 热重载重启会丢掉在途 invoke 的应答（promise 永不
    // settle），30s 强制超时转成错误，让上层给出重试入口而不是永远空态
    queryFn: () =>
      Promise.race([
        series.episodes(seriesId!),
        new Promise<never>((_, reject) => {
          setTimeout(() => reject(new Error('分集档案解析超时')), 30_000);
        }),
      ]),
    enabled: seriesId !== null,
  });
}

/**
 * 一部剧最近看到的那一集（本地 playback 表，5 秒一写的真值）。
 *
 * 故意不给 staleTime：详情页每次挂载都要现读——「继续看第 N 集」停在旧集
 * 的根源就是云端历史既滞后又有缓存，这里必须是本地最新值。
 */
export function useSeriesProgress(seriesId: string) {
  return useQuery({
    queryKey: keys.seriesProgress(seriesId),
    queryFn: () => play.progress(seriesId),
    enabled: seriesId !== '',
    gcTime: 60_000,
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
  /** 最近一次数据落定时间（毫秒；过期判断用） */
  dataUpdatedAt: number;
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
    /** 最近一次数据落定时间（过期判断用，如回首页换一批） */
    dataUpdatedAt: query.dataUpdatedAt,
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
 * 首页推荐流（书城换一换，无限滚动）。
 *
 * 翻页三件套回传（hgplayer 同款，2026-10-08 抓包实锤）：首页会话
 * `sessionId`（bookmall/tab cr=4 下发）+ `offset`（上一页 nextOffset，
 * 0→6→12 递进）+ `filterIds`（已下发过的 series_id，服务端排除已见）。
 *
 * pages 存在 Query 缓存里：切到其它路由再回来秒出已拉内容，不闪骨架屏；
 * staleTime 内完全不重打，超时只后台刷新（配合 RefreshShade 无感过渡）。
 */
export function useFeed(tab: string) {
  const queryClient = useQueryClient();
  const query = useInfiniteQuery({
    queryKey: keys.feed(tab),
    queryFn: ({ pageParam }) =>
      discover.recommendFeed(tab, pageParam.offset, pageParam.sessionId, pageParam.filterIds),
    initialPageParam: { offset: 0, sessionId: '', filterIds: [] as string[] },
    getNextPageParam: (last, allPages) => {
      if (!last.hasMore) return undefined;
      // filterIds 累积全部已下发条目（跨页去重的服务端形态）
      const seen = Array.from(new Set(allPages.flatMap((p) => p.items.map((i) => i.seriesId))));
      return { offset: last.nextOffset, sessionId: last.sessionId, filterIds: seen };
    },
    staleTime: 5 * 60_000,
  });
  const stream = useInfiniteStream(query, feedItems);
  // 换一批：重开会话回第 1 批（hgplayer V() 同款）。reset 会清掉已翻的页，
  // 重新从首页游标拉——原地 refetch 只会重放旧会话，看起来「没反应」。
  const restart = useCallback(() => {
    void queryClient.resetQueries({ queryKey: keys.feed(tab) });
  }, [queryClient, tab]);
  return { ...stream, restart };
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

/**
 * 一集的评论区（无限翻页：第一页秒回，面板底部「加载更多」续拉；
 * total 取自第一页的 need_count 回传，失败时报错由面板显示不打断播放）。
 */
export function useComments(vid: string) {
  const groupId = vid.split(':')[0] ?? '';
  const bookId = vid.split(':')[1] ?? '';
  return useInfiniteQuery({
    queryKey: keys.comments(vid),
    queryFn: ({ pageParam }) => danmakuCmd.comments(groupId, bookId, pageParam),
    initialPageParam: '',
    getNextPageParam: (last) => (last.hasMore && last.nextCursor ? last.nextCursor : undefined),
    enabled: vid.includes(':'),
    staleTime: 60_000,
  });
}

/** 发评论：成功后乐观插入第一页顶部，评论总数 +1。 */
export function useSendComment() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { vid: string; text: string }) => {
      const [groupId, bookId] = input.vid.split(':');
      return interactCmd.sendComment(groupId ?? '', bookId ?? '', input.text);
    },
    onSuccess: (commentId, input) => {
      queryClient.setQueryData<InfiniteData<CommentPage, string>>(
        keys.comments(input.vid),
        (prev) => {
          if (!prev) return prev;
          const [first, ...rest] = prev.pages;
          if (!first) return prev;
          return {
            ...prev,
            pages: [
              {
                ...first,
                total: first.total + 1,
                items: [
                  {
                    commentId,
                    userName: '我',
                    avatar: '',
                    text: input.text,
                    createTime: Math.floor(Date.now() / 1000),
                    diggCount: 0,
                    replyCount: 0,
                    userDigg: false,
                  },
                  ...first.items,
                ],
              },
              ...rest,
            ],
          };
        },
      );
    },
  });
}

/**
 * 回复一条评论（reply/add）。回复**列表**服务端暂无拉取接口
 * （2026-10-06 probe 实证：reply/list 对 aid 8662 无 handler，hgplayer
 * 同样不拉）——自己发的回复由评论面板本地追加展示。
 */
export function useSendReply() {
  return useMutation({
    mutationFn: (input: {
      vid: string;
      replyToCommentId: string;
      replyToReplyId?: string;
      text: string;
    }) => {
      const [groupId, bookId] = input.vid.split(':');
      return interactCmd.sendReply(
        groupId ?? '',
        bookId ?? '',
        input.replyToCommentId,
        input.replyToReplyId ?? null,
        input.text,
      );
    },
  });
}

/** 书架（我的收藏）列表；登录后才拉。收藏/取消收藏后要失效。 */
export function useBookshelf() {
  const { data: account } = useAccount();
  return useQuery({
    queryKey: keys.bookshelf,
    queryFn: interactCmd.bookshelf,
    enabled: !!account,
    staleTime: 60_000,
  });
}

/**
 * 剧集元信息（收藏/点赞等列表页用）：本地档案命中秒回，未收录的
 * （如书架里从没看过的剧）回落 resolve_series 解析并进同一份缓存。
 */
export function useSeriesMeta(seriesId: string) {
  return useQuery({
    queryKey: keys.seriesEpisodes(seriesId),
    queryFn: async () => {
      try {
        return await series.episodes(seriesId);
      } catch {
        return series.resolve(seriesId);
      }
    },
    enabled: seriesId !== '',
    staleTime: 10 * 60_000,
  });
}

/**
 * 登录/退出后的统一缓存刷新：账号态、互动回显、书架、预约一起失效。
 * 任何登录成功/退出入口都该调（LoginDialog / 侧边栏账户区），否则
 * 互动栏与列表页要等 staleTime 过期才翻面。
 */
export function useAuthRefresh() {
  const qc = useQueryClient();
  return useCallback(() => {
    void qc.invalidateQueries({ queryKey: keys.account });
    void qc.invalidateQueries({ queryKey: keys.interactState });
    void qc.invalidateQueries({ queryKey: keys.bookshelf });
    void qc.invalidateQueries({ queryKey: RESERVATIONS_KEY_ROOT });
  }, [qc]);
}

/** 点赞 / 取消点赞一集。 */
export function useVideoDigg() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { vid: string; seriesId: string; digg: boolean }) =>
      interactCmd.videoDigg(input.vid, input.seriesId, input.digg),
    // 乐观更新：互动回显接口（ugc/action/mget）是「最近互动列表」，
    // 剧集不在列表里时状态永远是 false——不乐观写的话按钮永不点亮、
    // 二次点击也不会走取消分支（详情页「二次点击还是提示已收藏」事故）
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: keys.interactState });
      const prev = queryClient.getQueryData<InteractionState>(keys.interactState);
      queryClient.setQueryData<InteractionState>(keys.interactState, (old) => {
        const items = old?.items ?? [];
        const idx = items.findIndex((i) => i.vid === input.vid);
        const patched = { ...items[idx], userDigg: input.digg } as InteractionItem;
        return {
          items:
            idx >= 0
              ? items.toSpliced(idx, 1, patched)
              : [
                  ...items,
                  {
                    vid: input.vid,
                    seriesId: input.seriesId,
                    userDigg: input.digg,
                    diggedCount: 0,
                    followed: false,
                    followedCnt: 0,
                    seriesTitle: '',
                  },
                ],
        };
      });
      return { prev };
    },
    onError: (_e, _input, ctx) => {
      if (ctx?.prev) queryClient.setQueryData(keys.interactState, ctx.prev);
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.interactState }),
  });
}

/** 收藏 / 取消收藏一部剧。 */
export function useSeriesCollect() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { seriesId: string; collect: boolean }) =>
      interactCmd.seriesCollect(input.seriesId, input.collect),
    // 乐观写 followed（详情页/互动栏的收藏态都从 interactState 匹配），
    // 权威数据源是书架列表（onSuccess 里失效重拉）
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey: keys.interactState });
      const prev = queryClient.getQueryData<InteractionState>(keys.interactState);
      queryClient.setQueryData<InteractionState>(keys.interactState, (old) => {
        const items = old?.items ?? [];
        const idx = items.findIndex((i) => i.seriesId === input.seriesId);
        const patched = { ...items[idx], followed: input.collect } as InteractionItem;
        return {
          items:
            idx >= 0
              ? items.toSpliced(idx, 1, patched)
              : [
                  ...items,
                  {
                    vid: '',
                    seriesId: input.seriesId,
                    userDigg: false,
                    diggedCount: 0,
                    followed: input.collect,
                    followedCnt: 0,
                    seriesTitle: '',
                  },
                ],
        };
      });
      return { prev };
    },
    onError: (_e, _input, ctx) => {
      if (ctx?.prev) queryClient.setQueryData(keys.interactState, ctx.prev);
    },
    onSuccess: () => {
      // 收藏态的权威回显走书架列表；interactState 的回显是 best-effort，
      // 不失效它——服务端列表延迟回带旧值会把乐观态顶回去（同一事故）
      void queryClient.invalidateQueries({ queryKey: keys.bookshelf });
    },
  });
}

/**
 * 预约 / 取消预约一部剧（复用 2026-10-04 抓包的 uncover_subscribe 端点）。
 *
 * 成功后两件事：刷新预约列表（权威源）；**就地翻新榜单缓存里该条的
 * reserved 位**——cell 接口的 online_subscribed 是响应时刻快照，不翻新的
 * 话切个子榜回来（10 分钟 staleTime 内命中缓存）预约态就「丢」了。
 */
export function useReserveSeries() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: { seriesId: string; reserve: boolean }) =>
      rank.reserve(input.seriesId, input.reserve),
    onSuccess: (_data, input) => {
      void queryClient.invalidateQueries({ queryKey: RESERVATIONS_KEY_ROOT });
      for (const query of queryClient.getQueryCache().findAll({ queryKey: ['rank'] })) {
        queryClient.setQueryData<RankPage>(query.queryKey, (page) =>
          page
            ? {
                ...page,
                items: page.items.map((i) =>
                  i.seriesId === input.seriesId ? { ...i, reserved: input.reserve } : i,
                ),
              }
            : page,
        );
      }
    },
  });
}

// ---------------------------------------------------------------- 封面增强

/**
 * HEIC 封面的本地转码代理地址（hongguo-cover 协议，后端 ffmpeg 转 JPEG）。
 *
 * URL 形态按平台：Windows 用 `http://{scheme}.localhost`（WebView2 拦截约定），
 * macOS/Linux 用 `{scheme}://localhost`（WebKit 拦真 scheme）——与 Rust 侧
 * `protocol::scheme_base` 同一套规矩，给错的表现是封面全挂。
 */
function coverProxyUrl(remote: string): string {
  const bytes = new TextEncoder().encode(remote);
  let bin = '';
  for (const b of bytes) bin += String.fromCharCode(b);
  const b64 = btoa(bin).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '');
  const base = isWindows() ? 'http://hongguo-cover.localhost' : 'hongguo-cover://localhost';
  return `${base}/c/${b64}`;
}

/**
 * 封面增强：能直接渲染的格式给 undefined（组件原样加载源图）；HEIC 等
 * WebView 解不了的源给 hongguo-cover 本地转码代理地址（ffmpeg 下载 HEIC
 * 转 JPEG，磁盘缓存）。
 *
 * 官网 webp 兜底链已整体拆除（2026-10-08：数据面一律走官方 App 接口），
 * 封面只有「原图 / 本地代理」两条路，纯同步无查询。都失败时组件自己的
 * onError 占位图兜底。
 */
export function useWebCover(sourceCover: string) {
  return {
    data:
      sourceCover !== '' && !isRenderableCover(sourceCover)
        ? coverProxyUrl(sourceCover)
        : undefined,
  };
}

/** WebView2 能直接渲染的封面格式（与后端 is_renderable_cover 同口径）。 */
export function isRenderableCover(url: string): boolean {
  const path = url.split('?')[0] ?? url;
  const lower = path.toLowerCase();
  if (lower.endsWith('.webp') || lower.endsWith('.png') || lower.endsWith('.jpg') || lower.endsWith('.jpeg')) {
    return true;
  }
  // HEIC：Windows 的 WebView2 没有 HEIF 扩展就解不了，必须走代理；
  // macOS 的 WKWebView 原生可解（系统能力，VT/ImageIO 一层），直挂原图。
  if (lower.endsWith('.heic') && navigator.platform.toUpperCase().includes('MAC')) {
    return true;
  }
  return false;
}

// ---------------------------------------------------------------- 排行榜 / 新剧

/**
 * 一个榜单（内容tab × 子榜 × 筛选 组合缓存；榜单一天更新几次，10 分钟内
 * 不重打。切筛选时用 keepPreviousData 保住旧列表，避免整页闪 loading）。
 *
 * 无限滚动（**2026-10-09 抓 hgplayer 滚动榜单实锤的协议**）：每页固定
 * 20 条（limit 参数恒 "0" 不参与），首页 offset=0 不带 session_id；翻页
 * offset=响应的 next_offset（步进 10）并回传响应的 session_id。相邻页
 * 重叠 10 条——按 seriesId 去重。tabs 取首页（选项表每页随行下发）。
 */
export function useRank(selected: string, sub: string, panel: string) {
  const query = useInfiniteQuery({
    queryKey: keys.rank(selected, sub, panel),
    queryFn: ({ pageParam }) =>
      rank.list(selected, sub, panel, pageParam.offset, pageParam.sessionId),
    initialPageParam: { offset: 0, sessionId: '' },
    getNextPageParam: (last) =>
      last.hasMore && last.nextOffset > 0
        ? { offset: last.nextOffset, sessionId: last.sessionId }
        : undefined,
    staleTime: 10 * 60_000,
    placeholderData: keepPreviousData,
  });

  const items = useMemo(() => {
    const seen = new Set<string>();
    const out: RankItem[] = [];
    for (const page of query.data?.pages ?? []) {
      for (const item of page.items) {
        if (seen.has(item.seriesId)) continue;
        seen.add(item.seriesId);
        out.push(item);
      }
    }
    return out;
  }, [query.data]);
  const tabs = query.data?.pages[0]?.tabs ?? [];

  return {
    items,
    tabs,
    isLoading: query.isPending,
    isFetching: query.isFetching,
    error: query.error,
    refetch: () => query.refetch(),
    hasMore: query.hasNextPage,
    isFetchingMore: query.isFetchingNextPage,
    loadMore: () => void query.fetchNextPage().catch(() => undefined),
  };
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

/** 找剧筛选面板：八行维度选项（选项表随服务端运营变化，拉一次长期用）。 */
export function useBrowsePanel() {
  return useQuery({
    queryKey: keys.browsePanel,
    queryFn: discover.browsePanel,
    staleTime: Infinity,
  });
}

/** 找剧筛选流（无限滚动）。
 *
 * 翻页必须回传首页发放的 `sessionId`（服务端按它记住筛选上下文）+
 * 上一页的 `nextOffset` 游标——2026-10-07 抓 hgplayer 1.1.6 实证：
 * limit=18、offset 0→18→36…、session_id 从第二页起同值回传。
 * 旧的 (page-1)*18 算术 offset + 空串 session_id 会让结果集换源，
 * 表现就是「条数和第三方对不上」。
 */
export function useBrowseFeed(filters: BrowseFilters) {
  const query = useInfiniteQuery({
    queryKey: keys.browseFeed(filters),
    queryFn: ({ pageParam }) =>
      pageParam.offset === 0
        ? discover.browsePage(filters, 0)
        : discover.browsePage(filters, pageParam.offset, pageParam.sessionId),
    initialPageParam: { offset: 0, sessionId: '' },
    getNextPageParam: (last) =>
      last.hasMore ? { offset: last.nextOffset, sessionId: last.sessionId } : undefined,
    staleTime: 30_000,
  });
  return useInfiniteStream(query, feedItems);
}

/** 详情页相关作品·系列（失败静默降级，不阻塞推荐 tab 的其他内容）。 */
export function useRelatedSeries(seriesId: string) {
  return useQuery({
    queryKey: keys.relatedSeries(seriesId),
    queryFn: () => series.related(seriesId),
    staleTime: 10 * 60_000,
    retry: false,
  });
}

/**
 * 详情页头部元信息（追剧数/播放量/季徽/题材标签/备案号）。
 * 失败静默降级：头部缺这几行不影响主功能（与后端同一口径）。
 * （与上面收藏/列表页的 useSeriesMeta 不同：那个回退 resolve 拿整档案。）
 */
export function useSeriesDetailMeta(seriesId: string) {
  return useQuery({
    queryKey: keys.seriesMeta(seriesId),
    queryFn: () => series.meta(seriesId),
    staleTime: 10 * 60_000,
    retry: false,
  });
}

/**
 * 剧级评论（详情页「剧评」tab：整部剧一条线，group_type=1 形态；
 * total 就是 tab 上的「剧评 579」计数）。与单集评论区（播放器 💬）分库。
 */
export function useSeriesComments(seriesId: string) {
  return useInfiniteQuery({
    queryKey: keys.seriesComments(seriesId),
    queryFn: ({ pageParam }) => danmakuCmd.seriesComments(seriesId, pageParam),
    initialPageParam: '',
    getNextPageParam: (last) => (last.hasMore && last.nextCursor ? last.nextCursor : undefined),
    enabled: !!seriesId,
    staleTime: 60_000,
  });
}

/**
 * 发剧评（详情页「剧评」评论框）。
 *
 * 成功后失效剧评缓存：第一页重取，新评论按时间排序自然置顶。
 */
export function useSendSeriesReview(seriesId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (text: string) => danmakuCmd.seriesReviewSend(seriesId, text),
    onSuccess: () =>
      void queryClient.invalidateQueries({ queryKey: keys.seriesComments(seriesId) }),
  });
}

/** 预取一部剧的分集档案（本地缺失会回落解析并落库）——沉浸流切下一部剧时
 * resolve 链路提前走完，切换只剩取流时间。 */
export function usePrefetchSeriesEpisodes() {
  const qc = useQueryClient();
  return useCallback(
    (seriesId: string) => {
      if (!seriesId) return;
      void qc.prefetchQuery({
        queryKey: keys.seriesEpisodes(seriesId),
        queryFn: () => series.episodes(seriesId),
        staleTime: 5 * 60_000,
      });
    },
    [qc],
  );
}

export function useResolveSeries() {
  return useMutation({ mutationFn: (input: string) => series.resolve(input) });
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
  // 后端按 1% 步进推送（集内回调在后端打了闸），载荷里 percent/episodeCount
  // 都已填好且同步落了库；就地合并只是让界面即时跟上，不依赖下次重查。
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

/** 按剧聚合的磁盘占用（清理页剧列表，只含磁盘上真有文件的剧）。 */
export function useStorageSeries() {
  return useQuery({ queryKey: keys.storageSeries, queryFn: storage.seriesUsage });
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
    void qc.invalidateQueries({ queryKey: keys.storageSeries });
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

/**
 * 搜索联想（hgplayer 1.1.6 同款）：输入即查，30s 缓存重复输入秒出。
 *
 * 防抖在调用侧（组件里 300ms 才把词递进来），这里只管查；
 * 失败后端已吞掉返回空表，下拉无感消失。
 */
export function useSearchSuggest(q: string) {
  const kw = q.trim();
  return useQuery({
    queryKey: ['search-suggest', kw] as const,
    queryFn: () => seriesSearch.suggest(kw),
    // 至少 2 个字才值得打接口（单字联想噪声大）
    enabled: kw.length >= 2,
    staleTime: 30_000,
    retry: false,
  });
}

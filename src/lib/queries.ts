import { useCallback, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  browse,
  discover,
  download,
  merge,
  play,
  search,
  series,
  settings,
  storage,
  transcode,
  danmaku as danmakuCmd,
} from './ipc/commands';
import { useEvent } from './ipc/events';
import { EVENTS } from './ipc/types';
import type {
  DownloadProgress,
  DownloadTask,
  FeedItem,
  FeedPage,
  MergeMode,
  MergeTask,
  QueueStatus,
} from './schema';

/**
 * TanStack Query 的 key 工厂。
 *
 * 所有 key 必须从这里出，不要在组件里手写字符串——否则失效（invalidate）
 * 时容易漏掉某个 key，导致界面不刷新。
 */
const keys = {
  settings: ['settings'] as const,
  seriesList: ['series-list'] as const,
  seriesEpisodes: (id: string) => ['series-episodes', id] as const,
  seriesExtras: (id: string) => ['series-extras', id] as const,
  playbackHistory: ['playback-history'] as const,
  tasks: ['download-tasks'] as const,
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
  webCover: (seriesId: string) => ['web-cover', seriesId] as const,
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

/**
 * 推荐信息流的手动翻页累积器。
 *
 * 不走 useQuery 缓存：分页是「不断往后拼」的会话流，缓存键要么爆炸
 * （每页一个 key）要么丢上下文（只有最后一页）。这里自己持状态：
 * pages 累积、nextOffset 前进、错误就地可重试。
 */
export function useFeed() {
  const [pages, setPages] = useState<FeedPage[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 防抖并发的加载令牌：refresh 与 loadMore 竞争时旧请求的结果要作废
  const tokenRef = useRef(0);

  const load = useCallback(
    async (mode: 'first' | 'more') => {
      const token = ++tokenRef.current;
      if (mode === 'first') setLoading(true);
      else setLoadingMore(true);
      setError(null);
      const offset = mode === 'first' ? 0 : (pages.at(-1)?.nextOffset ?? 0);
      try {
        const page = await discover.feed(offset);
        if (token !== tokenRef.current) return; // 已被更新的请求取代
        setPages((prev) =>
          mode === 'first'
            ? [page]
            : // 服务端偶发跨页重复（推荐位轮换），按 seriesId 去重
              [...prev, page],
        );
      } catch (e) {
        if (token !== tokenRef.current) return;
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        if (token === tokenRef.current) {
          setLoading(false);
          setLoadingMore(false);
        }
      }
    },
    [pages],
  );

  /** 全部已拉取条目，按拉取顺序、seriesId 去重。 */
  const items: FeedItem[] = [];
  const seen = new Set<string>();
  for (const page of pages) {
    for (const item of page.items) {
      if (seen.has(item.seriesId)) continue;
      seen.add(item.seriesId);
      items.push(item);
    }
  }

  return {
    items,
    hasMore: pages.at(-1)?.hasMore ?? false,
    isLoading: loading,
    isFetchingMore: loadingMore,
    error,
    loadMore: () => load('more'),
    refresh: () => load('first'),
  };
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

// ---------------------------------------------------------------- 封面增强

/** webp 封面懒加载：HEIC 源只在 HEVC 扩展齐全的机器上能显示，这里换成官网版。 */
export function useWebCover(seriesId: string, sourceCover: string) {
  const needs = !isRenderableCover(sourceCover);
  return useQuery({
    queryKey: keys.webCover(seriesId),
    queryFn: () => discover.webCover(seriesId),
    enabled: needs && seriesId !== '',
    staleTime: Infinity,
    gcTime: 30 * 60_000,
  });
}

/** WebView2 能直接渲染的封面格式（与后端 is_renderable_cover 同口径）。 */
function isRenderableCover(url: string): boolean {
  const path = url.split('?')[0] ?? url;
  const lower = path.toLowerCase();
  return (
    lower.endsWith('.webp') ||
    lower.endsWith('.png') ||
    lower.endsWith('.jpg') ||
    lower.endsWith('.jpeg')
  );
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

export function usePlaybackHistory() {
  return useQuery({ queryKey: keys.playbackHistory, queryFn: play.history });
}

export function useClearPlaybackHistory() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: play.clearHistory,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: keys.playbackHistory });
    },
  });
}

/** 清除某一部剧的观看记录。 */
export function useRemovePlaybackRecord() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (seriesId: string) => play.removeRecord(seriesId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: keys.playbackHistory });
    },
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

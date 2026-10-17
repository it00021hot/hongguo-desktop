/**
 * TanStack Query 查询层统一出口（按域拆分）。
 *
 * 页面从这里拿全部服务端数据 hooks；key 工厂与无限滚动封装在 common。
 */
export { keys } from './common';
export { useSettings, useSaveSettings, useTestProxy } from './settings';
export {
  useSeriesEpisodes,
  useSeriesProgress,
  useSeriesMeta,
  useRelatedSeries,
  useSeriesDetailMeta,
  usePrefetchSeriesEpisodes,
  useResolveSeries,
} from './series';
export { useFeed, useFeedTabs } from './feed';
export {
  useDanmaku,
  useSendDanmaku,
  useComments,
  useSendComment,
  useSendReply,
  useCommentReplies,
  useCommentDigg,
  useReplyDigg,
  useReviewReplies,
  useSeriesComments,
  useSendSeriesReview,
  useReviewDigg,
  useReviewReplyDigg,
  useSendReviewReply,
  useMentionList,
  useCreatorProfile,
  useCreatorWorks,
} from './danmaku';
export {
  useInteractionState,
  useBookshelf,
  useAuthRefresh,
  useVideoDigg,
  useSeriesCollect,
  useSeriesCollectBatch,
  useAccount,
} from './interact';
export {
  useFollowUpdates,
  useIsFollowing,
  useFollowToggle,
  useFollowNotices,
  useFollowNoticesMutations,
  useFollowCheck,
  useFollowNoticesInvalidator,
  unreadNotices,
} from './follow';

export {
  useRank,
  useNewDrama,
  useNewCalendar,
  useReservations,
  useReservationsFillAll,
  useReservationsDelete,
  useReserveSeries,
} from './rank';
export { useBrowsePanel, useBrowseFeed } from './browse';
export { useSeriesSearchApp, useSearchSuggest } from './search';
export {
  useDownloadTasks,
  useQueueStatus,
  useDownloadActions,
  useDownloadEvents,
} from './download';
export {
  useMergeTasks,
  useMergeCandidates,
  useMergePreflight,
  useMergeActions,
  useMergeEvents,
} from './merge';
export {
  usePlay,
  useSavePosition,
  useDecodeCapability,
  usePlaybackCapability,
  useInstallHevcExtension,
  useRedetectCapability,
  useCompatPlayback,
} from './play';
export {
  useStorageUsage,
  useStorageSeries,
  useCacheUsage,
  useCacheClear,
  useStorageActions,
} from './storage';
export {
  useLocalHistory,
  useLocalHistoryDelete,
  useWatchHistory,
  useWatchHistoryDelete,
} from './history';

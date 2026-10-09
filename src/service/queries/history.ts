import { useQuery } from '@tanstack/react-query';

import { watchHistory } from '../commands/watch-history';
import { keys } from './common';

// ---------------------------------------------------------------- 云端观看历史

/** 云端观看历史（官方 App「历史」同源；登录后可用，匿名回空表）。 */
export function useWatchHistory() {
  return useQuery({
    queryKey: keys.watchHistory,
    queryFn: () => watchHistory.list(),
    staleTime: 30_000,
  });
}

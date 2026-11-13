import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { watchHistory } from '../commands';
import type { HistoryAccount } from '../schema/history';
import { keys } from './common';
import { useAccount } from './interact';

export function useWatchHistory() {
  const { data: account } = useAccount();
  const expected = historyAccount(account);
  return useQuery({
    queryKey: keys.watchHistoryFor(expected?.userId ?? null, expected?.loginAt ?? null),
    queryFn: () => watchHistory.list(expected),
    enabled: account !== undefined,
  });
}

export function useWatchHistoryDelete() {
  const queryClient = useQueryClient();
  const { data: account } = useAccount();
  const expected = historyAccount(account);
  return useMutation({
    // 部分失败时组件层有带明细的定制 toast，全局错误 toast 静默
    meta: { silentOnError: true },
    mutationFn: (items: Parameters<typeof watchHistory.delete>[0]) =>
      watchHistory.delete(items, expected),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: keys.watchHistory }),
  });
}

function historyAccount(account: HistoryAccount | null | undefined): HistoryAccount | null {
  return account ? { userId: account.userId, loginAt: account.loginAt } : null;
}

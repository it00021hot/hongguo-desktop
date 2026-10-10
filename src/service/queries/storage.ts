import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { storage } from '../commands';
import { keys } from './common';

// ---------------------------------------------------------------- 存储

export function useStorageUsage() {
  return useQuery({ queryKey: keys.storageUsage, queryFn: storage.usage });
}

/** 按剧聚合的磁盘占用（清理页剧列表，只含磁盘上真有文件的剧）。 */
export function useStorageSeries() {
  return useQuery({ queryKey: keys.storageSeries, queryFn: storage.seriesUsage });
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

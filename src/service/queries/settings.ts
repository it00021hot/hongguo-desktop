import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { settings } from '../commands';
import { keys } from './common';

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

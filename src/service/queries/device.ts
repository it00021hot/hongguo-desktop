import { useCallback } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { device } from '../commands/device';
import { useEvent } from '../tauri/events';
import { EVENTS } from '../tauri/types';
import { keys } from './common';

export function useDeviceStatus() {
  const client = useQueryClient();
  const refresh = useCallback(() => {
    void client.invalidateQueries({ queryKey: keys.deviceStatus });
  }, [client]);
  useEvent(EVENTS.deviceRegistrationState, refresh);
  useEvent(EVENTS.deviceRegistered, refresh);
  return useQuery({
    queryKey: keys.deviceStatus,
    queryFn: device.status,
    meta: { silentOnError: true },
    retry: false,
  });
}

export function useRetryDeviceRegistration() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: device.retryRegistration,
    meta: { silentOnError: true },
    onSuccess: (status) => client.setQueryData(keys.deviceStatus, status),
  });
}

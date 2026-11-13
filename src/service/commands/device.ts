import { call } from '../tauri/invoke';

export const device = {
  status: () => call('device_status'),
  retryRegistration: () => call('device_retry_register'),
};

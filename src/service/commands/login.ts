import { call } from '../tauri/invoke';

export const login = {
  sendCode: (mobile: string) => call('login_send_code', { mobile }),
  smsLogin: (mobile: string, code: string, mfa?: { retryTag: string; smsCodeKey: string }) =>
    call('login_sms_login', {
      mobile,
      code,
      mfaRetryTag: mfa?.retryTag ?? null,
      mfaSmsCodeKey: mfa?.smsCodeKey ?? null,
    }),
  mfaVerify: () => call('login_mfa_verify'),
  /** 取消进行中的 MFA 验证（后台轮询随之停止） */
  mfaCancel: () => call('login_mfa_cancel'),
  status: () => call('login_status'),
  userInfo: () => call('login_user_info'),
  logout: () => call('login_logout'),
};

import { call } from '../tauri/invoke';
import {
  accountStateSchema,
  loginResultSchema,
  passportUserSchema,
  sendCodeOutcomeSchema,
  type AccountState,
  type LoginResult,
  type PassportUser,
  type SendCodeOutcome,
} from '../schema';

// ---------------------------------------------------------------- 登录

export const login = {
  sendCode: (mobile: string) =>
    call<SendCodeOutcome>('login_send_code', { mobile }, sendCodeOutcomeSchema),
  smsLogin: (
    mobile: string,
    code: string,
    ticket?: string,
    mfa?: { retryTag: string; smsCodeKey: string },
  ) =>
    call<LoginResult>(
      'login_sms_login',
      {
        mobile,
        code,
        mobileTicket: ticket ?? null,
        mfaRetryTag: mfa?.retryTag ?? null,
        mfaSmsCodeKey: mfa?.smsCodeKey ?? null,
      },
      loginResultSchema,
    ),
  mfaVerify: () => call<LoginResult>('login_mfa_verify', undefined, loginResultSchema),
  /** 取消进行中的 MFA 验证（后台轮询随之停止） */
  mfaCancel: () => call<void>('login_mfa_cancel', undefined),
  status: () => call<AccountState | null>('login_status', undefined, accountStateSchema.nullable()),
  userInfo: () => call<PassportUser>('login_user_info', undefined, passportUserSchema),
  logout: () => call<void>('login_logout'),
};

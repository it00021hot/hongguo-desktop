/** 登录域 zod schema（与 Rust serde 模型对应）。 */
import { z } from 'zod';
import { pageImageSrc } from '@/utils/offline-qa';

export const passportUserSchema = z.object({
  userId: z.string(),
  name: z.string(),
  mobile: z.string(),
  avatarUrl: z.string().catch('').transform(pageImageSrc),
});

/** 发码结果：mobileTicket 登录时必须回传 */
export const sendCodeOutcomeSchema = z.object({
  message: z.string(),
  mobileTicket: z.string(),
  /** 重发等待秒数（服务端 retry_time） */
  retryTime: z.number().default(60),
});

/**
 * MFA 后台轮询事件负载（`login-mfa-state`，Rust `login_service::events::LoginMfaStateEvent`）：
 * success = 自动登录完成；failed = 失败/超时（带 i18n key）；waiting = 新一轮上行短信验证提示。三态互斥。
 */
export const loginMfaStateSchema = z.discriminatedUnion('state', [
  z.object({ state: z.literal('success'), name: z.string() }),
  z.object({
    state: z.literal('failed'),
    /** 前端 i18n key（`settings.loginMfaTimeout` / `error.*`） */
    messageKind: z.string(),
    /** 后端原文（无对应词条时回落展示；超时这类纯业务态为空串） */
    message: z.string(),
  }),
  z.object({
    state: z.literal('waiting'),
    /** 服务端下发的回复提示，原样透传 */
    serverTip: z.string(),
  }),
]);

/** 登录命令返回：成功 / 需要 MFA 上行短信验证 / MFA 等待中 */
export const loginResultSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('success'), user: passportUserSchema }),
  z.object({
    kind: z.literal('mfa'),
    retryTag: z.string(),
    smsCodeKey: z.string(),
    /** 上行短信通道号（如 9515211003；提示文案已拼进 tips） */
    channelMobile: z.string().default(''),
    /** 要回复的短信内容（如 "YZ"） */
    smsContent: z.string().default(''),
    tips: z.string(),
  }),
  z.object({ kind: z.literal('mfaWaiting') }),
]);

export type LoginResult = z.infer<typeof loginResultSchema>;

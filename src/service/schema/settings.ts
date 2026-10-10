/**
 * 与 Rust serde 模型对应的 zod schema（设置域）。
 */
import { z } from 'zod';

const namingTemplateSchema = z.enum(['titleIndex', 'titleIndexEpisode', 'onlyTitle']);

const proxyConfigSchema = z.object({
  mode: z.enum(['system', 'manual', 'direct']),
  url: z.string(),
});

export type ProxyConfig = z.infer<typeof proxyConfigSchema>;

/** 已登录账号的会话快照（后端 Settings.account） */
export const accountStateSchema = z.object({
  mobile: z.string(),
  cookies: z.string(),
  userName: z.string(),
  // 旧库/旧后端无此字段：宽松兜空串
  avatarUrl: z.string().catch(''),
  userId: z.string(),
  loginAt: z.number(),
  /** x-tt-token 长凭据（旧账号为空） */
  token: z.string().catch(''),
  /** sms_login 响应原文（JSON；红果号等长尾字段按需读取，旧账号为空） */
  rawLogin: z.string().catch(''),
  /** user_info 响应原文（JSON；含 biz_user_id 红果号 / vip_info 等） */
  rawProfile: z.string().catch(''),
});

export type AccountState = z.infer<typeof accountStateSchema>;

export const settingsSchema = z.object({
  downloadDir: z.string(),
  naming: namingTemplateSchema,
  maxConcurrency: z.number().int().min(1).max(10),
  proxy: proxyConfigSchema,
  autoDeleteAfterPlay: z.boolean(),
  autoNextEpisode: z.boolean(),
  // 后端 serde(default)：旧库无此字段，前端宽松接收
  pauseOnMinimize: z.boolean().catch(true),
  theme: z.string(),
  // 后端 serde(default)：旧库无此字段，前端宽松接收
  account: accountStateSchema.nullable().optional(),
});

export type Settings = z.infer<typeof settingsSchema>;

export const proxyTestResultSchema = z.object({
  ok: z.boolean(),
  elapsedMs: z.number().nonnegative(),
  message: z.string(),
});

export type ProxyTestResult = z.infer<typeof proxyTestResultSchema>;

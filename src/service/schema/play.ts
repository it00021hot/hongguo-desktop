/**
 * 与 Rust serde 模型对应的 zod schema（播放与转码域）。
 */
import { z } from 'zod';

/** 一档清晰度。 */
export const videoDefinitionSchema = z.object({
  value: z.number().int().positive(),
  width: z.number().int().nonnegative(),
  height: z.number().int().nonnegative(),
});

export type VideoDefinition = z.infer<typeof videoDefinitionSchema>;

export const playResponseSchema = z.object({
  url: z.string(),
  online: z.boolean(),
  resumeAt: z.number().nonnegative(),
  error: z.string(),
  // 实际生效的档位与本集可选的全部档位。
  // 本地文件播放时 definitions 为空数组，前端据此隐藏切换菜单。
  definition: z.number().int().nonnegative(),
  definitions: z.array(videoDefinitionSchema),
});

export type PlayResponse = z.infer<typeof playResponseSchema>;

/** 转码能力：探测「这台机器会走哪条路」。 */
export const decodeCapabilitySchema = z.object({
  hasFfmpeg: z.boolean(),
  h264HwEncoder: z.boolean(),
  /** 平台原生编码层可用（macOS：VideoToolbox 会话，含 Apple 软编；Windows：硬件 MFT） */
  platformEncoder: z.boolean(),
  /** 平台原生**硬编**——只决定徽标「硬件加速」档，不是 macOS 生产闸门 */
  platformHwEncoder: z.boolean(),
});

/**
 * 在线播放的取流/解密进度。
 *
 * 整集取回 + 解密期间界面上原本只有一个转圈，用户既看不出在动还是卡住，
 * 也看不到还要多久。这个事件把「已收 / 总量 / 百分比 / 阶段」送上来。
 */
export const onlineProgressSchema = z.object({
  /** 缓存键，格式是 `{seriesId}:{vidIndex}` */
  key: z.string(),
  received: z.number(),
  /** CDN 没给 Content-Length 时为 0，此时不显示百分比 */
  total: z.number(),
  percent: z.number(),
  /** downloading | decrypting | ready */
  phase: z.enum(['downloading', 'decrypting', 'ready']),
});
export type OnlineProgress = z.infer<typeof onlineProgressSchema>;

/** 播放兼容兜底的转码进度。 */
export type CompatProgress = {
  /** 缓存键，格式 {seriesId}:{vidIndex} */
  key: string;
  percent: number;
  /** downloading | transcoding | ready */
  phase: 'downloading' | 'transcoding' | 'ready';
};

export type DecodeCapability = z.infer<typeof decodeCapabilitySchema>;

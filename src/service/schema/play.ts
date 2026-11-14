/** 播放与转码域 zod schema（与 Rust serde 模型对应）。 */
import { z } from 'zod';

const videoDefinitionSchema = z.object({
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
  /** `error` 非空时它的种类（Rust `AppError` 的 i18n key），否则为空串。 */
  errorKind: z.string().default(''),
  // 实际生效的档位与本集可选的全部档位。
  // 本地文件播放时 definitions 为空数组，前端据此隐藏切换菜单。
  definition: z.number().int().nonnegative(),
  automaticDefinition: z.boolean(),
  definitions: z.array(videoDefinitionSchema),
  /** 选中流编码，本地探测失败时为空串。 */
  codec: z.string().default(''),
  /** 这条流交给内置软解直播放（HEVC / ByteVC1 / ByteVC2），而不是喂给 `<video>`。 */
  direct: z.boolean().default(false),
});

export const decodeCapabilitySchema = z.object({
  /** 平台原生编码层可用（macOS：VideoToolbox 会话，含 Apple 软编；Windows：硬件 MFT） */
  platformEncoder: z.boolean(),
  /** 平台原生**硬编**——只决定徽标「硬件加速」档，不是 macOS 生产闸门 */
  platformHwEncoder: z.boolean(),
});

/** 在线播放的取流/解密进度：把「已收/总量/百分比/阶段」送上来，替代只有一个转圈的等待。 */
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

export type DecodeCapability = z.infer<typeof decodeCapabilitySchema>;

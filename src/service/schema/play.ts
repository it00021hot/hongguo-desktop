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
  // 实际生效的档位与本集可选的全部档位。
  // 本地文件播放时 definitions 为空数组，前端据此隐藏切换菜单。
  definition: z.number().int().nonnegative(),
  definitions: z.array(videoDefinitionSchema),
  /** 选中流编码；本地文件为空串 */
  codec: z.string().default(''),
  /** WebView 不能直喂（如 bytevc2），必须先兼容转码 */
  needsCompat: z.boolean().default(false),
});

export const decodeCapabilitySchema = z.object({
  hasFfmpeg: z.boolean(),
  h264HwEncoder: z.boolean(),
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

/** 播放兼容兜底的转码进度。 */
export const compatProgressSchema = z.object({
  /** 缓存键，格式 {seriesId}:{vidIndex} */
  key: z.string(),
  percent: z.number(),
  /** downloading | transcoding | ready | completed | failed */
  phase: z.enum(['downloading', 'transcoding', 'ready', 'completed', 'failed']),
});

export type CompatProgress = z.infer<typeof compatProgressSchema>;

export type DecodeCapability = z.infer<typeof decodeCapabilitySchema>;

/** 本机 WebView2 能否解 HEVC（决定无扩展机器的选流与扩展安装引导）。 */
export const playbackCapabilitySchema = z.boolean();

/** HEVC 扩展安装结果：安装后的能力重探值。 */
export const hevcInstallResultSchema = z.boolean();

/** 播放链路的转码决策结果（本地直读 / 缓存命中 / 实时转码）。 */
export const transcodePlaybackSchema = z.object({
  url: z.string(),
  cached: z.boolean(),
  backend: z.string(),
  elapsedMs: z.number(),
  /** 首段已可播，后续字节还在同一地址上增长 */
  progressive: z.boolean().default(false),
});

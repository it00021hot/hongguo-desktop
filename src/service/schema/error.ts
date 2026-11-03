/** 错误契约 zod schema（与 Rust serde 模型对应）。 */
import { z } from 'zod';

/** Rust 侧 AppError 的序列化形状。 */
export const appErrorSchema = z.object({
  kind: z.string(),
  message: z.string(),
});

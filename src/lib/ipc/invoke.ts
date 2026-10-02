import { invoke } from '@tauri-apps/api/core';
import { appErrorSchema } from '../schema';
import { t, tf } from '@/i18n';

/**
 * 类型化 invoke 封装。
 *
 * Rust 侧成功返回数据、失败返回 `{ kind, message }`。这里统一把错误转成
 * `Error`，让 TanStack Query 的 `error` 分支能直接拿到可展示的 message。
 */
export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
  schema?: { parse: (v: unknown) => T },
): Promise<T> {
  try {
    const raw = await invoke(command, args);
    return schema ? schema.parse(raw) : (raw as T);
  } catch (e) {
    throw toError(e, command);
  }
}

/** 把 Tauri 抛出的各种形态归一成 Error。 */
function toError(e: unknown, command: string): Error {
  // Rust 的自定义错误会被序列化成 { kind, message }：
  // kind 是 i18n key，message 是后端写死的中文原文。展示用译文，
  // 原文挂在 cause 上，英文界面下也还能靠它排查问题。
  if (e && typeof e === 'object' && 'kind' in e && 'message' in e) {
    const parsed = appErrorSchema.safeParse(e);
    if (parsed.success) {
      const localized = t(parsed.data.kind);
      const error = new Error(localized === parsed.data.kind ? parsed.data.message : localized);
      error.cause = parsed.data.message;
      return error;
    }
  }
  if (e instanceof Error) return e;
  if (typeof e === 'string') return new Error(e);
  return new Error(tf('error.unknownCommand', { command }));
}

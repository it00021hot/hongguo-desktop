import { invoke } from '@tauri-apps/api/core';
import { appErrorSchema } from '../schema';

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
export function toError(e: unknown, command: string): Error {
  // Rust 的自定义错误会被序列化成 { kind, message }
  if (e && typeof e === 'object' && 'kind' in e && 'message' in e) {
    const parsed = appErrorSchema.safeParse(e);
    if (parsed.success) {
      return new Error(parsed.data.message);
    }
  }
  if (e instanceof Error) return e;
  if (typeof e === 'string') return new Error(e);
  return new Error(`调用 ${command} 失败`);
}

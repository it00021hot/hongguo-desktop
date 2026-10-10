import { invoke } from '@tauri-apps/api/core';
import { appErrorSchema } from '../schema';
import { t } from '@/locales';

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
  // 传输层错误才走 AppError 归一；校验失败单独处理，两条路径不能混在
  // 一个 try 里——否则 zod 抛的 ZodError 会被当成后端错误再包一层。
  const raw = await invoke(command, args).catch((e: unknown) => {
    throw toError(e);
  });
  if (!schema) return raw as T;

  // ZodError 的 message 是后端响应体的 JSON 投影（path / code / expected），
  // 糊到 toast 上等于把内部数据契约摊给用户。换成一句能读懂的提示，
  // 原始 issue 挂到 cause 上，出问题时照样能查。
  try {
    return schema.parse(raw);
  } catch (e) {
    throw new Error(t('error.badPayload'), { cause: e });
  }
}

/** 把 Tauri 抛出的各种形态归一成 Error。 */
function toError(e: unknown): Error {
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
  // 认不出来的抛出物：文案只说「操作失败」，command 名属于内部信息，
  // 原始载荷留给 cause。
  return new Error(t('error.internal'), { cause: e });
}

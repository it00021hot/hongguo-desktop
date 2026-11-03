import { invoke } from '@tauri-apps/api/core';
import { appErrorSchema } from '../schema';
import { t, type MessageKey } from '@/locales';
import { COMMANDS, type CallRest, type CommandName, type CommandResponse } from './registry';

/**
 * 类型化 invoke：契约表在 `./registry`（与 EVENTS 同为两端镜像）；响应过 zod 校验，
 * 失败即后端改了契约（`error.badPayload`，原始 issue 挂 cause）。
 */
export async function call<C extends CommandName>(
  command: C,
  ...rest: CallRest<C>
): Promise<CommandResponse<C>> {
  const raw: unknown = await invoke(command, rest[0]).catch((e: unknown) => {
    throw toError(e);
  });
  try {
    return COMMANDS[command].response.parse(raw) as CommandResponse<C>;
  } catch (e) {
    // ZodError 的 message 是内部契约投影，不糊给用户；原始 issue 挂 cause。
    throw new Error(t('error.badPayload'), { cause: e });
  }
}

function toError(e: unknown): Error {
  // Rust 自定义错误序列化为 { kind, message }：kind 是 i18n key，展示用译文；
  // message 原文挂 cause，英文界面下也能靠它排查。
  if (e && typeof e === 'object' && 'kind' in e && 'message' in e) {
    const parsed = appErrorSchema.safeParse(e);
    if (parsed.success) {
      const error = new Error(localizedMessage(parsed.data.kind, parsed.data.message));
      error.cause = parsed.data.message;
      return error;
    }
  }
  if (e instanceof Error) return e;
  if (typeof e === 'string') return new Error(e);
  // 认不出的抛出物：只给通用文案，原始载荷挂 cause。
  return new Error(t('error.internal'), { cause: e });
}

/**
 * 错误文案归一：kind 有对应词条用译文，否则回落 message 原文。kind 键集由 Rust 侧
 * AppError 变体定义，无法静态枚举才 cast；命令错误与事件失败载荷共用此策略。
 */
export function localizedMessage(kind: string, message: string): string {
  const localized = t(kind as MessageKey);
  return localized === kind ? message : localized;
}

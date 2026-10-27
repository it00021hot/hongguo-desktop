import { useEffect } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { z } from 'zod';
import {
  compatProgressSchema,
  downloadProgressSchema,
  downloadTaskSchema,
  followNoticesEventSchema,
  loginMfaStateSchema,
  mergeTaskSchema,
  onlineProgressSchema,
} from '../schema';
import type { EventName } from './types';

/**
 * 事件载荷注册表：事件名（kebab-case 值，即 `EVENTS` 的值）→ zod schema。
 *
 * 与 `COMMANDS` 同性质的「两端镜像」——Rust 侧改载荷字段，**同一 commit**
 * 改这里。事件坏掉是静默的（比 IPC 更隐蔽），所以载荷一律过校验：
 * 不匹配时 console.error 并丢弃，绝不把形状不对的数据放进订阅方
 * （不报错说明契约没漂，漂了当场就在控制台）。
 */
const EVENT_PAYLOADS = {
  'download-progress': downloadProgressSchema,
  'download-task-added': downloadTaskSchema,
  'download-completed': z.object({ id: z.string() }),
  'download-failed': z.object({ id: z.string(), error: z.string() }),
  'download-stopped': z.object({ id: z.string() }),
  'download-queue-changed': z.object({}),
  'merge-progress': mergeTaskSchema,
  'merge-task-added': mergeTaskSchema,
  'merge-completed': mergeTaskSchema,
  'merge-failed': mergeTaskSchema,
  'online-play-progress': onlineProgressSchema,
  'compat-play-progress': compatProgressSchema,
  'login-mfa-state': loginMfaStateSchema,
  'follow-notices': followNoticesEventSchema,
  /** 设备注册换档完成：载荷 = 新 install_id（身份敏感数据应重拉） */
  'device-registered': z.string(),
  'boss-key-hidden': z.object({ hidden: z.boolean() }),
  /** lib.rs CloseRequested 拦截发出，Rust 侧 `()` 序列化为 null */
  'close-requested': z.null(),
  'incognito-visibility': z.object({ visible: z.boolean() }),
  /** lib.rs Resized+is_minimized 状态翻转发出：true=最小化 */
  'window-minimize': z.boolean(),
} as const satisfies Record<EventName, z.ZodType>;

/** 事件载荷（经 schema 校验后的形状）。 */
export type EventPayload<C extends EventName> = z.infer<(typeof EVENT_PAYLOADS)[C]>;

/**
 * 订阅后端事件并在组件卸载时自动清理。
 *
 * 载荷先过 `EVENT_PAYLOADS` 校验再进 handler；形状不匹配 console.error
 * 并丢弃该次事件。handler 请用 `useCallback` 包裹，否则每次渲染都会
 * 退订重订。
 */
export function useEvent<C extends EventName>(
  name: C,
  handler: (payload: EventPayload<C>) => void,
): void {
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    const schema = EVENT_PAYLOADS[name];
    void listen(name, (event) => {
      const parsed = schema.safeParse(event.payload);
      if (!parsed.success) {
        // 契约漂移是「后端改了载荷」，必须当场可查：事件名 + zod issue。
        console.error(`[events] ${name} 载荷校验失败`, parsed.error.issues, event.payload);
        return;
      }
      handler(parsed.data as EventPayload<C>);
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlisten = fn;
      }
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [name, handler]);
}

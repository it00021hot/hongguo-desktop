import { useEffect } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { z } from 'zod';
import {
  downloadProgressSchema,
  downloadTaskSchema,
  followNoticesEventSchema,
  loginMfaStateSchema,
  mergeTaskSchema,
  onlineProgressSchema,
} from '../schema';
import type { EventName } from './types';

/**
 * 事件载荷注册表（与 COMMANDS 同为两端镜像）：Rust 侧改载荷字段，同一 commit 改这里。
 * 载荷一律过校验，不匹配时 console.error 并丢弃，绝不把形状不对的数据放进订阅方。
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

export type EventPayload<C extends EventName> = z.infer<(typeof EVENT_PAYLOADS)[C]>;

/**
 * 载荷先过 `EVENT_PAYLOADS` 校验再进 handler；handler 须用 useCallback 包裹，
 * 否则每次渲染都会退订重订。
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

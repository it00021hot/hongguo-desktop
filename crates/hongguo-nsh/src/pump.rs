//! 业务事件泵：`ServiceEvent` 总线（tokio broadcast）→ nsh 信号。
//!
//! service 只发布强类型事件（`hongguo_service::events`），怎么送到 UI 由壳
//! 决定：Tauri 壳经 `events.rs` 转成前端事件，GPUI 壳用 `events.rs` 事件泵 +
//! `bridge.rs` oneshot 桥两件套。nsh 自带 tokio 与信号，**一个转发任务**即可
//! ——`background()` 就是 `tokio::spawn`，信号是 `Copy` 句柄，可在引擎线程
//! 上写、在任何视图里读。
//!
//! 转发任务只能在这里写信号：信号是 nsh 的图节点，脱离引擎上下文写会 panic
//!（`no reactive runtime`）。所以任务用 nsh 的 `spawn`（引擎线程）而不是
//! service 的 runtime。

use std::sync::LazyLock;

use hongguo_service::events::ServiceEvent;
use nsh_core::ui::prelude::*;

/// 本壳取流的进度键（`prepare` 的 `progress_key` 参数）：事件泵只把
/// 这一路的进度接进 UI，预取等其它 key 的进度不进。
pub(crate) const PROGRESS_KEY: &str = "hongguo-nsh";

/// 壳关心的业务事件快照（界面上直接读的几项）。
///
/// 壳骨架阶段只留三项：「后台在干什么」类状态。列表类数据仍走各自的查询，
/// 事件只做增量与状态提示。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShellEvents {
    /// 在线取流进度（0..=100）；`None` 表示当前没有进行中的取流。
    pub online_percent: Option<f64>,
    /// 追更提醒的新增条数（打开设置/首页时清零）。
    pub follow_notices: usize,
    /// 登录 MFA 是否在等用户在手机上确认。
    pub login_mfa_pending: bool,
}

static EVENTS: LazyLock<Signal<ShellEvents>> =
    LazyLock::new(|| signal_with(ShellEvents::default(), SignalEquals::Never));

/// 壳事件的只读句柄（视图里读它）。
pub fn events() -> Signal<ShellEvents> {
    *EVENTS
}

/// 起转发任务（组件体里调一次；任务随组件树销毁）。
///
/// 滞后语义：broadcast 落后时丢旧事件并记日志——与 service 的约定一致
///（壳侧列表数据另有快照查询兜底）。
pub fn start() {
    let events = events();
    spawn(async move {
        let mut receiver = crate::state::state().events().subscribe();
        loop {
            match receiver.recv().await {
                Ok(event) => apply(events, event),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    log::warn!("[Events] 事件泵滞后，丢弃 {skipped} 条旧事件");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    log::info!("[Events] 业务事件总线已关闭，事件泵退出");
                    break;
                }
            }
        }
    });
}

fn apply(signal: Signal<ShellEvents>, event: ServiceEvent) {
    match event {
        ServiceEvent::OnlineProgress(progress) => {
            // 只认本壳自己那路取流（key 在 prepare 时约定）；预取等
            // 其它 key 的进度不进 UI。
            if progress.key != PROGRESS_KEY {
                return;
            }
            signal.update(|state| {
                // 100% 之后不再挂进度条（事件流还会来收尾的几条）。
                state.online_percent = (progress.percent < 100.0).then_some(progress.percent);
            });
        }
        ServiceEvent::FollowNotices(notices) => {
            let added = notices.notices.len();
            if added > 0 {
                signal.update(|state| state.follow_notices += added);
            }
        }
        ServiceEvent::LoginMfa(mfa) => {
            use hongguo_service::login_service::events::LoginMfaStateEvent;
            let waiting = matches!(mfa, LoginMfaStateEvent::Waiting { .. });
            if !waiting {
                log::info!("[Events] 登录 MFA 结束：{mfa:?}");
            }
            signal.update(|events| events.login_mfa_pending = waiting);
        }
        // 下载/合并/漫画进度由各自的页面按需订阅；壳骨架阶段不铺开。
        _ => {}
    }
}

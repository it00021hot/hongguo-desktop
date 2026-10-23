//! 事件接线：service 业务事件泵、播放器事件循环、控制区控件订阅。
//!
//! 三条消费循环都在构造期起一次，把异步事件落回视图状态（与 Tauri 壳的
//! 事件转发同源，一份业务实现两种 UI 适配）。

use super::*;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use gpui_kit::Entity;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use hongguo_service::events::ServiceEvent;
use tokio::sync::mpsc;

use crate::home::player::PlayerEvent;

impl HomeView {
    /// 订阅 service 业务事件（登录 MFA、下载状态）并落到视图状态。
    ///
    /// 事件泵把 tokio broadcast 转成 GPUI 侧可 await 的无界通道
    /// （见 [`crate::events`]）；这里起一条消费循环——与 Tauri 壳消费
    /// 同一批事件（一份业务实现，两种 UI 适配）。
    pub(super) fn subscribe_service_events(&mut self, cx: &mut Context<Self>) {
        let mut rx = crate::events::subscribe(&self.state);
        cx.spawn(async move |this, cx| {
            while let Some(event) = rx.recv().await {
                let ok = this
                    .update(cx, |view, cx| view.on_service_event(event, cx))
                    .is_ok();
                if !ok {
                    break; // 视图已销毁
                }
            }
        })
        .detach();
    }

    /// service 事件 → 视图状态。
    fn on_service_event(&mut self, event: ServiceEvent, cx: &mut Context<Self>) {
        match event {
            ServiceEvent::LoginMfa(ev) => self.on_login_mfa_event(ev, cx),
            // 下载面板开着时随事件即时刷新（面板本身另有 1s 轮询兜底）
            ServiceEvent::DownloadTaskAdded(_)
            | ServiceEvent::DownloadProgress(_)
            | ServiceEvent::DownloadCompleted(_)
            | ServiceEvent::DownloadFailed(_)
            | ServiceEvent::DownloadStopped(_)
            | ServiceEvent::DownloadQueueChanged(_) => {
                if self.downloads_open {
                    self.refresh_downloads(cx);
                }
            }
            ServiceEvent::OnlineProgress(_) => {}
            ServiceEvent::MergeTaskAdded(_)
            | ServiceEvent::MergeProgress(_)
            | ServiceEvent::MergeCompleted(_)
            | ServiceEvent::MergeFailed(_) => {}
            ServiceEvent::CompatProgress(_) => {}
            // 追更提醒：GPUI 壳的追更页未建，先记日志（通知已落本地库，
            // 页面就位后从这里接 UI 刷新）
            ServiceEvent::FollowNotices(ev) => {
                log::info!("[Follow] {} 条新更新提醒", ev.notices.len());
            }
        }
    }

    pub(super) fn subscribe_controls(&mut self, cx: &mut Context<Self>) {
        // 弹幕设置滑杆：Change 即生效，Release 落盘（音量滑杆同节奏）
        let dm_sliders = [
            (&self.dm_opacity_slider, 0u8),
            (&self.dm_font_slider, 1),
            (&self.dm_density_slider, 2),
            (&self.dm_area_slider, 3),
        ];
        for (slider, which) in dm_sliders {
            cx.subscribe(
                slider,
                move |this: &mut Self, _: Entity<SliderState>, event: &SliderEvent, cx| {
                    this.on_dm_slider_event(which, event, cx);
                },
            )
            .detach();
        }
        // 弹幕发送框：Enter 提交；输入即「用户在场」（唤醒控件）
        if let Some(input) = &self.danmaku_input {
            cx.subscribe(
                input,
                |this: &mut Self, _: Entity<InputState>, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.submit_danmaku(cx);
                    }
                    // 输入框改动（含 Delete/退格删字）= 用户在打字：控件该亮。
                    // 舞台的 on_key_down 也会做同一件事，但输入框**吃掉**的按键
                    // （Delete 等）根本到不了舞台——只靠舞台那条路，控件会在
                    // 输入框里打字时一直藏着（Tauri 侧靠 wakeOnInput 兜住）。
                    if matches!(event, InputEvent::Change) {
                        this.wake_controls(cx);
                    }
                },
            )
            .detach();
        }
        // 评论输入框：Enter 提交；输入即「用户在场」（唤醒控件）
        if let Some(input) = &self.comment_input {
            cx.subscribe(
                input,
                |this: &mut Self, _: Entity<InputState>, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.submit_comment(cx);
                    }
                    if matches!(event, InputEvent::Change) {
                        this.wake_controls(cx);
                    }
                },
            )
            .detach();
        }
        // 下载浮层的区间语法：输入即解析（Tauri `applyRange` 同款——
        // 部分能解析就采用能解析的部分，全部解析不出才标红）
        cx.subscribe(
            &self.download_range,
            |this: &mut Self, input: Entity<InputState>, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let text = input.read(cx).value().to_string();
                this.apply_download_range(&text, cx);
            },
        )
        .detach();
    }

    /// 播放器事件循环（每次起播一个，epoch 封口：换剧后陈旧事件自然断流）。
    /// 系统播放器只发低频控制事件（首帧/缓冲/播完/失败），逐条投递即可。
    pub(super) fn spawn_event_loop(
        &mut self,
        epoch: u64,
        epoch_shared: Arc<AtomicU64>,
        mut events: mpsc::Receiver<PlayerEvent>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.recv().await {
                // 纪元先行判定：切剧/离开首页后本循环的剩余事件全部作废。
                if epoch_shared.load(Ordering::Relaxed) != epoch {
                    return;
                }
                let ok = this
                    .update(cx, |view, cx| {
                        if view.epoch.load(Ordering::Relaxed) != epoch {
                            log::warn!(
                                "[Home] 事件被纪元弃置（loop={epoch}，view={}）——事件到得了 view 却进不去",
                                view.epoch.load(Ordering::Relaxed)
                            );
                            return;
                        }
                        view.on_player_event(event, cx);
                    })
                    .is_ok();
                if !ok {
                    log::warn!("[Home] 事件投递失败（视图已销毁），事件循环退出");
                    return;
                }
            }
        })
        .detach();
    }
}

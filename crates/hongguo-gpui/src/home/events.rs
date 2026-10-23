//! 事件接线：service 业务事件泵、播放器引擎事件循环、控制区控件订阅。
//!
//! 三条消费循环都在构造期起一次，把异步事件落回视图状态（与 Tauri 壳的
//! 事件转发同源，一份业务实现两种 UI 适配）。

use super::*;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use gpui_kit::Entity;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use hongguo_playback::PlayerEvent;
use hongguo_service::events::ServiceEvent;
use tokio::sync::mpsc;

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

    /// 引擎事件循环（每个引擎一个，epoch 封口：换剧后陈旧事件自然断流）。
    /// 像素转换（Windows 的 NV12→RGBA）在后台线程池执行、不挤占主线程，
    /// 纪元判定因此必须走在转换**之前**。
    pub(super) fn spawn_event_loop(
        &mut self,
        epoch: u64,
        epoch_shared: Arc<AtomicU64>,
        mut events: mpsc::Receiver<PlayerEvent>,
        cx: &mut Context<Self>,
    ) {
        // 转换句柄在循环外取一次：目标尺寸/适配模式是原子共享，后台转换
        // 直接读，不必每帧回主线程。
        let convert_handle = self.video_surface.conversion_handle();
        cx.spawn(async move |this, cx| {
            let mut diag = lag_diag::EventDiag::new();
            while let Some(first) = events.recv().await {
                let batch_started = Instant::now();
                // 纪元先行判定：切剧/离开首页后本循环的剩余事件全部作废，
                // 直接退场——以前是整帧转换完才在 update 里丢弃，切剧窗口
                // （恰好是 UI 最忙的时候）负载凭空翻倍
                if epoch_shared.load(Ordering::Relaxed) != epoch {
                    return;
                }
                // 合并积压：Frame 只保最新一批里的最后一帧（1080p 转换 ~50ms、
                // 上屏只认最新，旧帧转换纯属浪费），其余事件一个不丢、按序处理。
                // 抽干按 16 条封顶：病态积压下（转换跟不上、通道满载）一条
                // Frame 事件背一帧 NV12（~3MB），不封顶就是单轮上百 MB 的
                // batch 分配；多余的下一轮再收，通道背压照旧作用在解码端。
                let mut batch = Vec::with_capacity(4);
                batch.push(first);
                while batch.len() < 8
                    && let Ok(next) = events.try_recv()
                {
                    batch.push(next);
                }
                let last_frame = batch.iter().rposition(|e| matches!(e, PlayerEvent::Frame(_)));
                // 合并取证：这一批里被丢掉的帧（只保留最后一帧上屏）是宿主
                // 跟不上的直接证据——引擎可没丢，是 UI 侧没画。
                let batch_frames = batch
                    .iter()
                    .filter(|e| matches!(e, PlayerEvent::Frame(_)))
                    .count() as u32;
                diag.note_batch(batch.len(), batch_frames, batch_frames.saturating_sub(1));
                'batch: for (i, event) in batch.into_iter().enumerate() {
                    if matches!(event, PlayerEvent::Frame(_)) && Some(i) != last_frame {
                        continue;
                    }
                    // 像素载体转换（仅 Windows 需要：NV12→RGBA 上传纹理）。
                    // macOS 是 CVPixelBuffer 零拷贝，**必须完全不碰这条路径**——
                    // 每帧 spawn 到后台执行器再 await，在后台线程被别的任务
                    // 占住时会把整个事件循环卡死（实测：首帧过后画面就再也不
                    // 更新，引擎侧仍在 30fps 上屏）。
                    #[cfg(target_os = "windows")]
                    let converted = match &event {
                        PlayerEvent::Frame(frame) => {
                            let frame = frame.clone();
                            let handle = convert_handle.clone();
                            let t_convert = Instant::now();
                            let converted = cx
                                .background_executor()
                                .spawn(async move {
                                    video_surface::convert_frame(&handle, &frame)
                                })
                                .await;
                            diag.note_convert(t_convert.elapsed());
                            converted
                        }
                        _ => None,
                    };
                    #[cfg(not(target_os = "windows"))]
                    let converted: Option<Arc<gpui_kit::RenderImage>> = {
                        let _ = convert_handle;
                        None
                    };
                    let ok = this
                        .update(cx, |view, cx| {
                            if view.epoch.load(Ordering::Relaxed) != epoch {
                                log::warn!(
                                    "[Home] 事件被纪元弃置（loop={epoch}，view={}）——事件到得了 view 却进不去",
                                    view.epoch.load(Ordering::Relaxed)
                                );
                                return;
                            }
                            // 周期哨兵（debug）：这条涨而 `render #` 不涨 = 事件
                            // 到了、重绘没被调度（窗口遮挡/合成器不请求帧）。
                            // info 级的上屏节奏在 `[Home][paint]` 窗口摘要里。
                            static HANDLED: AtomicU32 = AtomicU32::new(0);
                            let h = HANDLED.fetch_add(1, Ordering::Relaxed);
                            if h.is_multiple_of(100) {
                                log::debug!(
                                    "[Home] 事件已处理 #{h}（epoch={epoch}，帧槽={}）",
                                    view.frame.is_some()
                                );
                            }
                            view.video_surface.adopt(converted);
                            if matches!(&event, PlayerEvent::Frame(_)) {
                                view.paint_diag.note_frame();
                            }
                            view.on_player_event(event, cx);
                        })
                        .is_ok();
                    if !ok {
                        log::warn!("[Home] 事件投递失败（视图已销毁），事件循环退出");
                        break 'batch;
                    }
                }
                // 这一批从出通道到全部投递完成（含 Windows 像素转换）的耗时：
                // 主线程被别的东西占住时，帧会在通道里排队，这里先涨。
                diag.note_lag(batch_started.elapsed());
                diag.report();
            }
        })
        .detach();
    }
}

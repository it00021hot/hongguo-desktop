//! 控制区行为：引擎事件分发、控制条显隐状态机、偏好落盘与唤醒。

use super::*;

use std::time::Instant;

#[cfg(feature = "screenshot")]
use std::time::Duration;

use hongguo_playback::PlayerEvent;

use crate::bridge;

impl HomeView {
    /// 取证辅助（screenshot feature 的 HONGGUO_SHOT_PAUSE）：暂停播放并把
    /// 控制条可见窗拉长到 10 分钟——无人值守抓帧才能拍到控制条/互动栏。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_pause_player(&mut self, cx: &mut Context<Self>) {
        if let Some(player) = &self.player
            && !player.is_paused()
        {
            player.pause();
        }
        self.controls_until = Some(Instant::now() + Duration::from_secs(CONTROLS_HOLD_SECS));
        cx.notify();
    }

    /// 取证辅助（HONGGUO_SHOT_PANEL=dm-settings）：打开弹幕设置面板并拉长
    /// 控制条可见窗（无人值守抓四行滑杆 UI）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_dm_settings(&mut self, cx: &mut Context<Self>) {
        self.dm_panel_open = true;
        self.controls_until = Some(Instant::now() + Duration::from_secs(CONTROLS_HOLD_SECS));
        cx.notify();
    }

    pub(in crate::home) fn on_player_event(&mut self, event: PlayerEvent, cx: &mut Context<Self>) {
        match event {
            PlayerEvent::FirstFrame { .. } => {
                if let Some(started) = self.system_switch_started.take() {
                    log::info!(
                        "[AvPresenter] 切集首帧 {:.0}ms（system 后端）",
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                }
                // 系统播放器的画面由 AVPlayerLayer 自己合成：首帧事件只置
                // 「画面已就绪」，视图据此打透明洞；GPUI 后端仍等 Frame。
                if self.player.as_ref().is_some_and(|p| p.is_system()) {
                    self.system_video_ready = true;
                    self.first_frame_generation = self.play_generation;
                    self.hold_last_frame = false;
                    // 系统后端没有后续 Frame 事件；这里必须主动调度一帧，
                    // 否则舞台永远停在上一帧的黑色/封面。
                    cx.notify();
                    // 取证：HONGGUO_AV_NEXT_EPISODE_MS=NNNN 首帧后 NN 秒自动切下一集。
                    if let Ok(ms) = std::env::var("HONGGUO_AV_NEXT_EPISODE_MS")
                        && let Ok(ms) = ms.parse::<u64>()
                    {
                        static ONCE: std::sync::atomic::AtomicBool =
                            std::sync::atomic::AtomicBool::new(false);
                        if !ONCE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                            cx.spawn(async move |this, cx| {
                                cx.background_executor()
                                    .timer(std::time::Duration::from_millis(ms))
                                    .await;
                                let _ = this.update(cx, |view, cx| view.step_episode(1, cx));
                            })
                            .detach();
                        }
                    }
                    // 取证：HONGGUO_AV_RATE=2 首帧后切倍速。
                    if let Ok(rate) = std::env::var("HONGGUO_AV_RATE")
                        && let Ok(rate) = rate.parse::<f64>()
                    {
                        self.set_rate(rate, cx);
                    }
                }
                // 帧由紧随其后的 Frame 事件带上；这里只做「出画了」的副作用
                self.prefetch_next_stream(cx);
                // 新集首帧出画 = 连播静默期结束（Tauri 首播解除静默同款）
                self.silent_until_input = false;
            }
            PlayerEvent::Frame(frame) => {
                let first = self.frame.is_none();
                if first {
                    log::info!("[Home] 首帧进入 view——video_ready 即为 true");
                }
                // 每个 Frame 都证明「当前代次已有画面」；同剧切集时 frame
                // 槽沿用旧帧，不能只在 first 时记代次。
                self.first_frame_generation = self.play_generation;
                // 新集首帧出画：定格帧交棒（Tauri FrameHold 淡出同款）
                self.hold_last_frame = false;
                self.frame = Some(frame);
                self.save_progress_maybe();
                // 弹幕 tick：入场 + 在飞推进（Frame 事件即渲染节拍）
                self.danmaku_tick(cx);
                // 相邻剧分集档案预取（换剧秒回，Tauri 同节奏）
                self.prefetch_neighbor_archives();
            }
            // 引擎缓冲状态机：水位低暂停设备、回填恢复（详见 playback::player）
            PlayerEvent::Buffering => self.buffering = true,
            PlayerEvent::Ready => self.buffering = false,
            PlayerEvent::Ended => {
                self.buffering = false;
                // 来源标记交给 advance_after_end 的代次幂等统一裁决。
                self.advance_after_end(cx, "ended");
            }
            PlayerEvent::Failed(msg) => {
                self.buffering = false;
                // 失败落封面：定格帧此时只会盖住错误态（Tauri 真值表 error 档）
                self.hold_last_frame = false;
                // 播放中致命失败同样走 retry 档（对齐 Tauri recovery-policy
                // 的 network/other：自动重试一次，复发才落错误态）——起播
                // 失败与这里共用同一个 play_retried 预算（引擎就位即复位）。
                // 引擎已停、时钟冻结在失败位置，读它就是续播断点。
                if !self.play_retried
                    && let Some(item) = self.display_items().get(self.cursor()).cloned()
                {
                    self.play_retried = true;
                    let position = self
                        .player
                        .as_ref()
                        .map(|h| h.position_secs())
                        .unwrap_or(0.0);
                    // 集号必须显式带上：start_override 单独在场时 play_task
                    // 会把集号回落到第 1 集（那语义是「切清晰度保进度」，
                    // 必然伴随显式集号），第 5 集失败就会被重试到第 1 集去
                    let vid_index = self.now.as_ref().map(|n| n.vid_index).unwrap_or(1);
                    log::warn!(
                        "[Home] 播放中失败，自动重试一次（第 {vid_index} 集 {position:.1}s 起）: {msg}"
                    );
                    self.play_target(item, Some(vid_index), Some(position), cx);
                    cx.notify();
                    return;
                }
                // 引擎致命失败即自停（shutdown），封面背后不会再有画面——
                // 失败必须落可见错误态：只写 pending_title 的话，那行小徽标
                // 随 switching 消失，用户看到的就是「无声无息卡在封面」
                self.play_error = Some(crate::i18n::tf(
                    "common.failedWith",
                    &[("message", msg.as_str())],
                ));
            }
        }
        cx.notify();
    }

    pub(in crate::home) fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        let Some(player) = &self.player else {
            return;
        };
        if player.is_paused() {
            player.resume();
            // 恢复播放重新持有活动断言（play_target 持有、用户暂停释放）
            crate::play_activity::set_active(true);
        } else {
            player.pause();
            // 暂停 = 用户在场，系统可以休眠；主流播放器同语义
            crate::play_activity::set_active(false);
            // 暂停 = force 落盘 + 补一条观看历史（Tauri persist(force) 同款）
            self.save_progress_now();
        }
        self.wake_controls(cx);
        cx.notify();
    }

    pub(crate) fn set_rate(&mut self, rate: f64, cx: &mut Context<Self>) {
        self.rate = rate;
        if let Some(player) = &self.player {
            player.set_rate(rate);
        }
        self.persist_prefs();
        self.wake_controls(cx);
        cx.notify();
    }

    /// 偏好落盘（数据目录键值文件，见 [`crate::playback_prefs`]）。
    pub(in crate::home) fn persist_prefs(&self) {
        let prefs = crate::playback_prefs::PlaybackPrefs {
            rate: self.rate,
            volume: self.volume,
            muted: self.muted,
            danmaku_on: self.danmaku_on,
            dm_opacity: self.dm_opacity,
            dm_font_scale: self.dm_font_scale,
            dm_density: self.dm_density,
            dm_area: self.dm_area,
            incognito: self.incognito,
            keys: self.shortcut_keys.clone(),
        };
        bridge::spawn(async move {
            let path = hongguo_store::paths::data_dir().join(crate::playback_prefs::FILE_NAME);
            let _ =
                tokio::task::spawn_blocking(move || crate::playback_prefs::save_to(&path, &prefs))
                    .await;
        });
    }

    /// 鼠标活动/操作刷新控制条可见窗；隐藏→可见的沿上排一次性定时器，
    /// 静止到期后靠它（或播放中的帧事件流）触发重算隐藏。真实输入同时
    /// 解除连播静默（Tauri wakeOnInput 同款）。
    pub(in crate::home) fn wake_controls(&mut self, cx: &mut Context<Self>) {
        // 静默期闸门（Tauri wakeOnMove 同款）：连播静默起步后，「用户在场」
        // 只认真实输入（点击/按键/滚轮——它们显式走这里之外的解除路径）与
        // 首帧；指针移动不算，不然静默刚起就被一次晃动解除，悬浮层全亮
        if self.silent_until_input {
            return;
        }
        let was_hidden = !self.controls_visible();
        self.controls_until = Some(Instant::now() + Self::CONTROLS_VISIBLE);
        if was_hidden {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Self::CONTROLS_VISIBLE).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            })
            .detach();
            cx.notify();
        }
    }

    /// 真实输入的「用户在场」：解除连播静默并亮悬浮层（Tauri
    /// pointerdown/按键 wakeChrome 同款——与 [`Self::wake_controls`] 的差
    /// 别就在静默期是否放行）。点击画面、键盘、滚轮切剧走这里。
    pub(in crate::home) fn wake_on_input(&mut self, cx: &mut Context<Self>) {
        self.silent_until_input = false;
        self.wake_controls(cx);
    }

    /// 任一面板打开即常显（Tauri panelOpen 同一集合：选集/评论/弹幕设置/
    /// 音量/下载）。倍速/清晰度下拉菜单与表情网格虽不是「面板」，但开着时
    /// 指针不在控制条本体上、hover 养不了常显窗——不进来就会被帧事件流的
    /// 重算连菜单一起收掉（播放中每帧都触发，暂停态没重渲染所以显得
    /// 「只有暂停能操作」）。
    fn panel_open(&self) -> bool {
        self.picker_open
            || self.comments_panel_open
            || self.dm_panel_open
            || self.volume_popup_open
            || self.downloads_open
            || self.rate_menu_open
            || self.definition_menu_open
            || self.danmaku_emoji_open
    }

    /// 悬浮层总闸（Tauri chromeShown 同款裁决）：连播静默期一票否决（面板
    /// 照常）；否则 悬停控制栏 / 拖进度 / 暂停 / 活动窗未到期 / 面板开 任一
    /// 即亮。指针离开舞台的立即收由舞台 on_hover 清 controls_until 实现。
    pub(in crate::home) fn controls_visible(&self) -> bool {
        if self.silent_until_input {
            return self.panel_open();
        }
        self.controls_hovered
            || self.scrubbing.is_some()
            || self.player.as_ref().is_some_and(|p| p.is_paused())
            || self.controls_until.is_some_and(|t| Instant::now() < t)
            || self.panel_open()
    }

    /// 隐身模式开关（Tauri incognito 同语义）：开 = 起光标轮询，鼠标脱离
    /// 窗口即暂停 + 藏窗，回到窗口矩形原地重现 + 续播；关 = 停轮询并带回
    /// 窗口。实现见 [`crate::incognito`]。
    pub(in crate::home) fn toggle_incognito(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        self.set_incognito(!self.incognito, window, cx);
    }
}

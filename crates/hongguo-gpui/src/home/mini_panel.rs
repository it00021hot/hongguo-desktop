//! 小屏播放：紧凑控制条渲染与同窗几何切换（进入/退出/关闭/钉住/循环倍速）。

use gpui_kit::component::Disableable;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{SharedString, div};

impl super::HomeView {
    /// 小屏紧凑控制条（Tauri MiniScreenControls 对齐）：剧名/集数/简介、
    /// 进度条、一行紧凑按钮——480×270 里只留最必要的控件。与大屏控制栏
    /// 共用同一套状态源（player handle / scrubbing / rate / muted）。
    pub(super) fn render_mini_controls(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let paused = self.player.as_ref().is_some_and(|p| p.is_paused());
        let duration = self
            .player
            .as_ref()
            .map(|p| p.duration_secs())
            .unwrap_or(0.0);
        let position = self.scrubbing.unwrap_or_else(|| {
            self.player
                .as_ref()
                .map(|p| p.position_secs())
                .unwrap_or(0.0)
        });
        let now = self.now.as_ref();
        let has_next = now.is_some_and(|n| (n.vid_index as usize) < n.episode_count);
        let episode_line = now.map(|n| {
            let mut s = crate::i18n::tf("player.episodeNo", &[("index", &n.vid_index.to_string())]);
            if n.episode_count > 0 {
                s.push_str(&format!(
                    " · {}",
                    crate::i18n::tf(
                        "player.totalEpisodes",
                        &[("count", &n.episode_count.to_string())],
                    )
                ));
            }
            if let Some(intro) = self.intros.get(&n.series_id) {
                s.push(' ');
                s.push_str(intro);
            }
            s
        });
        v_flex()
            .id("mini-controls")
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .px_3()
            .pt_5()
            .pb_1()
            .gap_1()
            .bg(gpui_kit::rgba(0x000000bf))
            // 与大屏控制条同款：小屏控制条悬浮在画面上，点击/松开都**不能**
            // 再落到舞台（重叠 bounds 的兄弟分支都会收到鼠标事件）
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _: &gpui_kit::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _: &gpui_kit::MouseUpEvent, _, cx| {
                    cx.stop_propagation();
                }),
            )
            // 悬在控制条本体上不倒计时（Tauri MiniScreenControls
            // onControlsEnter/Leave 同款，与大屏控制条同一份状态）
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.controls_hovered = *hovered;
                if *hovered {
                    this.wake_controls(cx);
                }
                cx.notify();
            }))
            // 剧名/集数/简介贴着进度条上方（第三方小屏同款两行形态），宽度
            // 收敛 + 截断（480 宽，长剧名长简介都不挡画面）
            .child(
                v_flex()
                    .max_w(gpui_kit::relative(0.62))
                    .overflow_hidden()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(gpui_kit::rgb(0xffffff))
                            .text_ellipsis()
                            .child(SharedString::from(format!(
                                "@{}",
                                now.map(|n| n.title.as_str()).unwrap_or("")
                            ))),
                    )
                    .children(episode_line.map(|line| {
                        div()
                            .text_size(gpui_kit::px(10.0))
                            .text_color(gpui_kit::rgba(0xffffffcc))
                            .text_ellipsis()
                            .child(SharedString::from(line))
                    })),
            )
            // 进度条与大屏共用同一轨道（拖动/点击/scrubbing 状态同源）
            .child(self.render_progress_track(cx))
            .child(
                h_flex()
                    .items_center()
                    .gap_0p5()
                    .child(
                        Button::new("mini-toggle-play")
                            .icon(if paused {
                                IconName::Play
                            } else {
                                IconName::Pause
                            })
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_pause(cx))),
                    )
                    .child(
                        Button::new("mini-prev-ep")
                            .icon(gpui_kit::assets::IconName::SkipBack)
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, _, cx| this.step_episode(-1, cx))),
                    )
                    .child(
                        Button::new("mini-next-ep")
                            .icon(gpui_kit::assets::IconName::SkipForward)
                            .ghost()
                            .xsmall()
                            .disabled(!has_next)
                            .on_click(cx.listener(|this, _, _, cx| this.step_episode(1, cx))),
                    )
                    .child(
                        div()
                            .ml_1()
                            .text_size(gpui_kit::px(10.0))
                            .font_family("monospace")
                            .text_color(gpui_kit::rgba(0xffffffd9))
                            .child(format!(
                                "{} / {}",
                                Self::fmt_secs(position),
                                Self::fmt_secs(duration)
                            )),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("mini-rate")
                            .label(Self::fmt_rate(self.rate))
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, _, cx| this.cycle_rate_mini(cx))),
                    )
                    .child(
                        Button::new("mini-mute")
                            .icon(if self.muted {
                                gpui_kit::assets::IconName::VolumeX
                            } else {
                                gpui_kit::assets::IconName::Volume2
                            })
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))),
                    )
                    .child(
                        Button::new("mini-incognito")
                            .icon(if self.incognito {
                                gpui_kit::assets::IconName::Eye
                            } else {
                                gpui_kit::assets::IconName::EyeOff
                            })
                            .ghost()
                            .xsmall()
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.toggle_incognito(window, cx)
                                }),
                            ),
                    )
                    .child(
                        Button::new("mini-pin")
                            .icon(if self.pinned {
                                gpui_kit::assets::IconName::Pin
                            } else {
                                gpui_kit::assets::IconName::PinOff
                            })
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_pinned(window, cx);
                            })),
                    )
                    .child(
                        Button::new("mini-expand")
                            .icon(gpui_kit::assets::IconName::Maximize2)
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.exit_mini(window, cx);
                            })),
                    )
                    .child(
                        Button::new("mini-close")
                            .icon(gpui_kit::assets::IconName::X)
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.stop_mini(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 进入小屏（Tauri enterMini 同款）：浮层全收 + **同一个窗口**缩成
    /// 480×270 落工作区右下角（`crate::mini`，播放零中断）。全屏先进
    /// 全屏退出——几何快照才有意义。
    pub(crate) fn enter_mini(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if self.mini.is_some() || self.player.is_none() {
            return;
        }
        // 竖屏的几何快照是单份的：进小屏前必须先把竖屏退掉，否则小屏快照
        // 记的是竖条几何（Tauri use-portrait-window 同款守卫）
        if self.portrait.is_some() {
            self.exit_portrait(window, cx);
        }
        if window.is_fullscreen() {
            window.toggle_fullscreen();
            self.fullscreen = false;
            // 从沉浸全屏直接进小屏：immersive 一并退，别留「无全屏却藏
            // chrome」的中间态
            self.immersive = false;
        }
        match crate::mini::enter(window, cx) {
            Some(saved) => {
                self.mini = Some(saved);
                crate::mini::set_active(true);
                // 大屏的浮层带不进 480×270 的小窗：进小屏前一并收掉
                //（Tauri enterMini 同款）。下拉菜单本体收不掉（Popover 随
                // 触发钮卸载自毁），这里清的是常显记账位
                self.comments_panel_open = false;
                self.dm_panel_open = false;
                self.downloads_open = false;
                self.picker_open = false;
                self.picker_group_override = None;
                self.volume_popup_open = false;
                self.danmaku_emoji_open = false;
                self.rate_menu_open = false;
                self.definition_menu_open = false;
                self.wake_controls(cx);
                log::info!("[Home] 进入小屏 480×270（工作区右下角）");
            }
            None => log::warn!("[Home] 进入小屏失败：窗口几何不可用，保持原状"),
        }
        cx.notify();
    }

    /// 退出小屏：按快照还原窗口几何（最大化态一并还原）。
    pub(super) fn exit_mini(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if let Some(saved) = self.mini.take() {
            crate::mini::exit(window, cx, saved);
            crate::mini::set_active(false);
            self.wake_controls(cx);
            log::info!("[Home] 退出小屏，窗口几何已还原");
        }
        cx.notify();
    }

    /// 关闭小屏（Tauri stopMini 同款：暂停 + 强落进度 + 还原窗口；gpui 壳
    /// 只有播放流一个根，无需再跳路由）。
    fn stop_mini(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if let Some(player) = &self.player
            && !player.is_paused()
        {
            player.pause();
        }
        self.save_progress_now();
        self.exit_mini(window, cx);
    }

    /// 小屏里点倍速标签循环档位（Tauri MINI_RATES 同款，完整菜单回大屏）。
    pub(super) fn cycle_rate_mini(&mut self, cx: &mut Context<Self>) {
        const MINI_RATES: [f64; 4] = [1.0, 1.5, 2.0, 3.0];
        let next = match MINI_RATES.iter().position(|r| (r - self.rate).abs() < 0.01) {
            Some(i) => MINI_RATES[(i + 1) % MINI_RATES.len()],
            None => MINI_RATES[0],
        };
        self.set_rate(next, cx);
    }
}

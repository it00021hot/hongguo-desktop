//! 竖屏播放：进入/退出/铺满档切换与竖屏形态的紧凑控制条（Tauri
//! `use-portrait-window` + `MiniScreenControls variant="portrait"` 对齐）。
//!
//! 窗口几何在 [`crate::portrait`]；这里只管播放侧状态与 UI：
//! - 比例取「实际在播的档位尺寸」（没有档位表就用解码帧尺寸），过手机形
//!   地板（`portrait::geom::aspect_of_video`）；
//! - 换集/换清晰度导致比例变化 >1% 时重适配（Tauri REFIT_DELTA 同值）；
//! - 竖屏里没有大屏控制栏：渲染紧凑条（无剧名块、无置顶，多一个铺满档）。

use gpui_kit::component::Disableable;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{SharedString, div};

use crate::portrait::geom;

/// 比例变化超过 1% 才重适配（Tauri REFIT_DELTA 同值）：浮点抖动与同比例
/// 换流不触发。
const REFIT_DELTA: f64 = 0.01;

impl super::HomeView {
    /// 当前竖屏比例：优先实际在播档位（档位表的宽高就是视频宽高），
    /// 没有就退解码帧尺寸，再没有回落手机形比例。
    pub(super) fn portrait_aspect(&self) -> f64 {
        let now = self.now.as_ref();
        if let Some(n) = now
            && let Some(d) = n.definitions.iter().find(|d| d.value == n.definition)
        {
            return geom::aspect_of_video(d.width as usize, d.height as usize);
        }
        if let Some(player) = &self.player {
            let (w, h) = player.dims();
            if w > 0 && h > 0 {
                return geom::aspect_of_video(w, h);
            }
        }
        geom::aspect_of_video(0, 0)
    }

    /// 竖屏开关（控制栏竖屏钮）。进竖屏先退小屏（两条紧凑形态的几何快照
    /// 是单份的，套娃会把快照写坏——Tauri 同守卫）。
    pub(crate) fn toggle_portrait(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        if self.portrait.is_some() {
            self.exit_portrait(window, cx);
            return;
        }
        if self.mini.is_some() {
            self.exit_mini(window, cx);
        }
        self.enter_portrait(window, cx);
    }

    /// 进入竖屏：几何快照 → 按比例收窄（Tauri `enterPortrait` + 后端
    /// `enter_portrait_screen` 同序：先退全屏，再落窗）。
    pub(crate) fn enter_portrait(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if self.portrait.is_some() || self.player.is_none() {
            return;
        }
        // 全屏窗口尺寸归系统管，收窄会被吞掉（Tauri 同款守卫）
        if window.is_fullscreen() {
            window.toggle_fullscreen();
            self.fullscreen = false;
            self.immersive = false;
        }
        let aspect = self.portrait_aspect();
        match crate::portrait::enter(window, aspect, cx) {
            Some(saved) => {
                self.portrait = Some(saved);
                self.portrait_full = false;
                self.portrait_aspect_last = aspect;
                crate::portrait::set_active(true);
                // 大屏浮层带不进竖条：进竖屏前一并收掉（Tauri enterPortrait 同款）
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
                log::info!("[Home] 进入竖屏（比例 {aspect:.3}）");
            }
            None => log::warn!("[Home] 进入竖屏失败：窗口几何不可用，保持原状"),
        }
        cx.notify();
    }

    /// 退出竖屏：还原进入前的几何与最大化态。
    pub(crate) fn exit_portrait(&mut self, window: &mut gpui_kit::Window, cx: &mut Context<Self>) {
        if let Some(saved) = self.portrait.take() {
            crate::portrait::exit(window, saved, cx);
            crate::portrait::set_active(false);
            self.portrait_full = false;
            self.wake_controls(cx);
            log::info!("[Home] 退出竖屏，窗口几何已还原");
        }
        cx.notify();
    }

    /// 铺满档切换（竖屏紧凑条的铺满钮；全屏键在竖屏里也让位给它）。
    pub(crate) fn toggle_portrait_full(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        if self.portrait.is_none() {
            return;
        }
        let next = !self.portrait_full;
        self.portrait_full = next;
        let aspect = self.portrait_aspect();
        crate::portrait::set_full(window, aspect, next, cx);
        self.wake_controls(cx);
        cx.notify();
    }

    /// 换集/换清晰度后的重适配：比例变化 >1% 才动窗口（Tauri `resize`
    /// 监听 + REFIT_DELTA 同口径）。不在竖屏中是空操作。
    pub(super) fn refit_portrait(&mut self, window: &gpui_kit::Window, cx: &gpui_kit::App) {
        if self.portrait.is_none() {
            return;
        }
        let aspect = self.portrait_aspect();
        if (aspect - self.portrait_aspect_last).abs() <= REFIT_DELTA {
            return;
        }
        self.portrait_aspect_last = aspect;
        crate::portrait::fit(window, aspect, self.portrait_full, cx);
    }

    /// 竖屏紧凑控制条（Tauri `MiniScreenControls variant="portrait"` 对齐）：
    /// 与小屏同一套按钮与状态源，差别是**无剧名/集数块**（竖条里剧名/简介
    /// 由信息层承担）、无置顶（顶栏常驻，置顶钮在顶栏/大屏控制栏）、多一个
    /// 铺满档，右端是「退出竖屏」。
    pub(super) fn render_portrait_controls(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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
        let has_next = self
            .now
            .as_ref()
            .is_some_and(|n| (n.vid_index as usize) < n.episode_count);
        v_flex()
            .id("portrait-controls")
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .px_2()
            .pt_5()
            .pb_1p5()
            .gap_1()
            .bg(gpui_kit::rgba(0x000000bf))
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
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.controls_hovered = *hovered;
                if *hovered {
                    this.wake_controls(cx);
                }
                cx.notify();
            }))
            .child(self.render_progress_track(cx))
            .child(
                h_flex()
                    .items_center()
                    .gap_0p5()
                    .child(
                        Button::new("portrait-toggle-play")
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
                        Button::new("portrait-prev-ep")
                            .icon(gpui_kit::assets::IconName::SkipBack)
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, _, cx| this.step_episode(-1, cx))),
                    )
                    .child(
                        Button::new("portrait-next-ep")
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
                            .whitespace_nowrap()
                            .child(SharedString::from(format!(
                                "{} / {}",
                                Self::fmt_secs(position),
                                Self::fmt_secs(duration)
                            ))),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("portrait-rate")
                            .label(Self::fmt_rate(self.rate))
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(|this, _, _, cx| this.cycle_rate_mini(cx))),
                    )
                    .child(
                        Button::new("portrait-mute")
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
                        Button::new("portrait-incognito")
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
                    // 铺满档（竖屏专属档位；小窗尺寸固定无档位可言）
                    .child(
                        Button::new("portrait-full")
                            .icon(if self.portrait_full {
                                gpui_kit::assets::IconName::Minimize
                            } else {
                                gpui_kit::assets::IconName::Maximize
                            })
                            .ghost()
                            .xsmall()
                            .tooltip(if self.portrait_full {
                                "退出铺满"
                            } else {
                                "铺满"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_portrait_full(window, cx);
                            })),
                    )
                    .child(
                        Button::new("portrait-exit")
                            .icon(gpui_kit::assets::IconName::RectangleHorizontal)
                            .ghost()
                            .xsmall()
                            .tooltip("退出竖屏")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.exit_portrait(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}

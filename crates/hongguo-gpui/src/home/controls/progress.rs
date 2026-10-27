//! 进度轨道：拖动 seek、实时节流与松手收口。

use super::*;

use std::rc::Rc;
use std::time::Instant;

use gpui_kit::component::h_flex;
use gpui_kit::div;

impl HomeView {
    /// 键盘相对 seek（秒，可负）：钳到 [0, 时长]，唤醒控制条。
    pub(in crate::home) fn seek_by(&mut self, secs: f64, cx: &mut Context<Self>) {
        let Some(player) = &self.player else {
            return;
        };
        let dur = player.duration_secs();
        let target = (player.position_secs() + secs).clamp(0.0, dur.max(0.0));
        player.seek(target);
        self.wake_controls(cx);
        cx.notify();
    }

    /// 进度轨道的指针位置 → 目标秒。轨道矩形缺失（首帧前）返回 `None`。
    fn scrub_target_at(&self, x: gpui_kit::Pixels) -> Option<f64> {
        let bounds = self.progress_bounds.get()?;
        let player = self.player.as_ref()?;
        let dur = player.duration_secs();
        if dur <= 0.0 {
            return None;
        }
        let ratio = ((x.to_f64() - bounds.origin.x.to_f64()) / bounds.size.width.to_f64().max(1.0))
            .clamp(0.0, 1.0);
        Some(ratio * dur)
    }

    /// 收口拖动：松手（轨道内/外都算）把预览位置交给引擎。松手的这次是
    /// 权威定位，**不受节流**——节流窗口内也要照发。
    fn commit_scrub(&mut self, cx: &mut Context<Self>) {
        if let Some(target) = self.scrubbing.take() {
            self.scrub_seek_now(target, cx);
        }
        self.wake_controls(cx);
        cx.notify();
    }

    /// 立即定位（不受节流）：按下即跳与松手收口共用。
    fn scrub_seek_now(&mut self, target: f64, cx: &mut Context<Self>) {
        self.last_scrub_seek.set(Instant::now());
        if let Some(player) = &self.player {
            player.seek(target);
        }
        self.wake_controls(cx);
        cx.notify();
    }

    /// 拖动中的实时定位（对齐 Tauri ScrubBar 的 pointermove 即 seek）。节流
    /// 窗口内的重复请求直接跳过——预览位置由 `scrubbing` 承载，不受影响；
    /// 松手收口走 [`Self::commit_scrub`]，不受此限。
    fn scrub_seek_throttled(&mut self, target: f64, cx: &mut Context<Self>) {
        if self.last_scrub_seek.get().elapsed() < Self::SCRUB_SEEK_MIN_INTERVAL {
            return;
        }
        self.last_scrub_seek.set(Instant::now());
        if let Some(player) = &self.player {
            player.seek(target);
        }
        self.wake_controls(cx);
        cx.notify();
    }

    /// 进度轨道（自绘）：细条填充按播放位置实时走（系统节拍驱动重渲染）；
    /// 悬停加粗并亮出滑块圆点（Tauri ScrubBar 同款）；点击即跳转，按住
    /// 拖动为预览（时间读数跟手），松手才 seek。轨道矩形经 prepainted
    /// 哨兵存取，拖动事件只带窗口坐标。
    pub(in crate::home) fn render_progress_track(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let duration = self
            .player
            .as_ref()
            .map(|p| p.duration_secs())
            .unwrap_or(0.0);
        // 显示位置钳在时长内：挂钟收尾时钟会轻微越过结尾（~1s），不钳的
        // 话时间读数会走出「043/0:59 → 1:01/0:59」这种对不上的观感
        let position = self
            .scrubbing
            .unwrap_or_else(|| {
                self.player
                    .as_ref()
                    .map(|p| p.position_secs())
                    .unwrap_or(0.0)
            })
            .min(duration.max(0.0));
        let frac = if duration > 0.0 {
            (position / duration).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bounds = Rc::clone(&self.progress_bounds);
        let hovered = self.progress_hovered;
        h_flex()
            .w_full()
            .items_center()
            .cursor_pointer()
            // 命中区加高（好点），视觉条只有 4px
            .h_4()
            .relative()
            .child(
                div()
                    .relative()
                    .w_full()
                    // 静止态 2px 半透明细条贴着画面，悬停/拖动涨到 4px 并
                    // 提亮（ScrubBar 同款「平时隐身、上手好用」）
                    .when(hovered, |s| s.h_1())
                    .when(!hovered, |s| s.h(gpui_kit::px(2.0)))
                    .rounded_full()
                    .bg(gpui_kit::rgba(0xffffff33))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .when(hovered, |s| s.h_1())
                            .when(!hovered, |s| s.h(gpui_kit::px(2.0)))
                            .w(gpui_kit::relative(frac as f32))
                            .rounded_full()
                            .bg(gpui_kit::rgba(if hovered {
                                0xffffffe6
                            } else {
                                0xffffff99
                            })),
                    )
                    // 滑块圆点（Tauri ScrubBar 同款：悬停轨道才亮）
                    .children(hovered.then(|| {
                        div()
                            .absolute()
                            .top(gpui_kit::px(-4.0))
                            .left(gpui_kit::relative(frac as f32))
                            .ml(gpui_kit::px(-6.0))
                            .size_3()
                            .rounded_full()
                            .bg(gpui_kit::rgb(0xffffff))
                    })),
            )
            // prepainted 哨兵是 Div 的方法：必须在 .id() 变 Stateful 之前挂
            .on_children_prepainted(move |children, _window, _cx| {
                // 第一个子层是视觉条；矩形存哨兵供指针位置换算
                if let Some(b) = children.first() {
                    bounds.set(Some(*b));
                }
            })
            .id("stage-progress-track")
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if this.progress_hovered != *hovered {
                    this.progress_hovered = *hovered;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, ev: &gpui_kit::MouseDownEvent, _window, cx| {
                    // 对齐 Tauri ScrubBar 的 pointerdown：按下即定位（不是只
                    // 起预览），随后拖动持续跟手
                    if let Some(target) = this.scrub_target_at(ev.position.x) {
                        this.scrubbing = Some(target);
                        this.scrub_seek_now(target, cx);
                    }
                }),
            )
            .on_drag(ScrubDrag, |_, _, _, cx| cx.new(|_| ScrubGhost))
            .on_drag_move::<ScrubDrag>(cx.listener(
                |this, ev: &gpui_kit::DragMoveEvent<ScrubDrag>, _window, cx| {
                    if let Some(target) = this.scrub_target_at(ev.event.position.x) {
                        this.scrubbing = Some(target);
                        // 实时 scrub：拖到哪放到哪（引擎侧节流限频）
                        this.scrub_seek_throttled(target, cx);
                    }
                },
            ))
            // 松手收口：轨道内/外都要算（拖出轨道再松手也是一次明确的定位）
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _: &gpui_kit::MouseUpEvent, _window, cx| {
                    this.commit_scrub(cx);
                }),
            )
            .on_mouse_up_out(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _: &gpui_kit::MouseUpEvent, _window, cx| {
                    this.commit_scrub(cx);
                }),
            )
            .into_any_element()
    }
}

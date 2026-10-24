//! 音量：竖向滑杆、弹层开合与静音切换。

use super::*;

use std::rc::Rc;

use gpui_kit::component::Sizable;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::v_flex;
use gpui_kit::div;

use crate::ui_button::button as Button;

impl HomeView {
    /// 音量滑杆：拖动即生效（音量值直设播放器，即时可闻），松手落盘。
    /// 音量弹层开合裁决（按钮 hover ∥ 弹层 hover ∥ 拖动中，见挂载处注释）。
    fn update_volume_popup(&mut self, cx: &mut Context<Self>) {
        let open = self.volume_hovered || self.volume_popup_hovered || self.volume_dragging;
        if open {
            self.wake_controls(cx);
        } else {
            // 收起时落一次盘：拖动过程中的音量值别丢
            self.persist_prefs();
        }
        if self.volume_popup_open != open {
            self.volume_popup_open = open;
            cx.notify();
        }
    }

    /// 音量滑杆的按住/松手：拖动期间把弹层钉住。
    ///
    /// gpui 进入拖拽捕获后不再按命中盒判 hover，弹层的 hover 位会当场
    /// 掉；hover 驱动的弹层一收，滑杆就从元素树里消失，drag_move 再也
    /// 送不到它——音量停在按下处，拖不动（用户实测）。
    fn set_volume_dragging(&mut self, dragging: bool, cx: &mut Context<Self>) {
        if self.volume_dragging == dragging {
            return;
        }
        self.volume_dragging = dragging;
        self.update_volume_popup(cx);
    }

    /// 指针在音量竖条上的取值（竖向：顶部 = 1；轨道矩形来自哨兵）。
    fn volume_ratio_at(&self, y: f64) -> f32 {
        match self.volume_bounds.get() {
            Some(b) => {
                let top = b.origin.y.to_f64();
                let height = b.size.height.to_f64().max(1.0);
                (1.0 - (y - top) / height).clamp(0.0, 1.0) as f32
            }
            None => 0.0,
        }
    }

    /// 设音量（拖/点音量条；静音独立，由喇叭钮切换——Tauri 同语义）。
    fn set_volume_value(&mut self, v: f32, cx: &mut Context<Self>) {
        self.volume = v.clamp(0.0, 1.0);
        if let Some(player) = &self.player {
            player.set_volume(self.volume);
        }
        self.wake_controls(cx);
        cx.notify();
    }

    /// 竖向音量条（Tauri VerticalSlider 同款三段自绘：轨道/已填/滑块）。
    /// 组件库 Slider 的竖向形态与播放器的白色系视觉不一致，这里照抄三段。
    fn render_volume_slider(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let value = (if self.muted { 0.0 } else { self.volume }).clamp(0.0, 1.0);
        let bounds = Rc::clone(&self.volume_bounds);
        div()
            // prepaint 哨兵是 Div 的方法：必须在 .id() 变 Stateful 之前挂
            .on_children_prepainted(move |children, _, _| {
                if let Some(b) = children.first() {
                    bounds.set(Some(*b));
                }
            })
            .id("volume-slider")
            .relative()
            .cursor_pointer()
            .w(gpui_kit::px(24.0))
            .h(gpui_kit::px(96.0))
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, ev: &gpui_kit::MouseDownEvent, _, cx| {
                    let v = this.volume_ratio_at(ev.position.y.to_f64());
                    this.set_volume_value(v, cx);
                    // 按住即进入拖动态：弹层钉住，drag_move 才送得到
                    this.set_volume_dragging(true, cx);
                }),
            )
            .on_drag(VolumeDrag, |_, _, _, cx| cx.new(|_| ScrubGhost))
            .on_drag_move::<VolumeDrag>(cx.listener(
                |this, ev: &gpui_kit::DragMoveEvent<VolumeDrag>, _, cx| {
                    let v = this.volume_ratio_at(ev.event.position.y.to_f64());
                    this.set_volume_value(v, cx);
                },
            ))
            // 松手收口：滑杆内/外都要算（拖出滑杆再松手也是一次明确的
            // 取值；留在拖动态里弹层就钉着，收不回去）
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _: &gpui_kit::MouseUpEvent, _, cx| {
                    this.set_volume_dragging(false, cx);
                }),
            )
            .on_mouse_up_out(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _: &gpui_kit::MouseUpEvent, _, cx| {
                    this.set_volume_dragging(false, cx);
                }),
            )
            // 轨道（w-1 居中）
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(gpui_kit::px(10.0))
                    .w(gpui_kit::px(4.0))
                    .rounded_full()
                    .bg(gpui_kit::rgba(0xffffff40)),
            )
            // 已填（自下而上）
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left(gpui_kit::px(10.0))
                    .w(gpui_kit::px(4.0))
                    .h(gpui_kit::relative(value))
                    .rounded_full()
                    .bg(gpui_kit::rgb(0xffffff)),
            )
            // 滑块（底部锚定 value 高度，负 margin 下移半格对中线）
            .child(
                div()
                    .absolute()
                    .bottom(gpui_kit::relative(value))
                    .left(gpui_kit::px(6.0))
                    .mb(gpui_kit::px(-6.0))
                    .size_3()
                    .rounded_full()
                    .bg(gpui_kit::rgb(0xffffff)),
            )
            .into_any_element()
    }

    pub(in crate::home) fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.muted = !self.muted;
        if let Some(player) = &self.player {
            player.set_muted(self.muted);
        }
        self.persist_prefs();
        self.wake_controls(cx);
        cx.notify();
    }

    /// 音量入口（Tauri VolumePopup 同款）：按钮单击静音切换，悬停向上弹滑杆。
    pub(super) fn render_volume_controls(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        // 音量（Tauri VolumePopup 同款）：图标按钮单击=静音切换；
        // 悬停向上弹竖向滑杆，弹层与按钮几何贴合，按钮/弹层两
        // 个 hover 取并集裁决开合（间隙 = 移过去就收的元凶），
        // 拖动中再加一位钉住（见 update_volume_popup）
        div()
            .id("volume-wrapper")
            .relative()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.volume_hovered = *hovered;
                this.update_volume_popup(cx);
            }))
            .child(
                Button::new("stage-mute")
                    .icon(if self.muted || self.volume == 0.0 {
                        gpui_kit::assets::IconName::VolumeX
                    } else if self.volume < 0.5 {
                        gpui_kit::assets::IconName::Volume1
                    } else {
                        gpui_kit::assets::IconName::Volume2
                    })
                    .custom(chrome_variant(cx))
                    .cursor_pointer()
                    .small()
                    .tooltip(crate::i18n::t("player.mute"))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(cx))),
            )
            .children(self.volume_popup_open.then(|| {
                // deferred：控制栏向上弹的浮层必须最后画（选集浮层同款）。
                // 不 deferred 就留在控制栏的普通绘制序里——后画的兄弟元素
                // （评论面板/连播横条/登录面板…）会盖住它，浮层只剩半截。
                let popup = v_flex()
                    .id("volume-popup")
                    .absolute()
                    // 贴合按钮上沿（bottom-full），视觉留白进
                    // 自己的 padding
                    .bottom(gpui_kit::relative(1.0))
                    // 居中在按钮上（Tauri left-1/2 -translate-x-1/2 同款）：
                    // 弹层盒宽 = 内容 24（滑杆）+ 左右 padding 24 = 48，
                    // 左偏移取一半 24 才真的居中（原先按 60 取 30，偏左 6px）
                    .left(gpui_kit::relative(0.5))
                    .ml(gpui_kit::px(-24.0))
                    .items_center()
                    .pt_1()
                    .pb_3()
                    .px_3()
                    .rounded(gpui_kit::px(8.0))
                    .bg(gpui_kit::rgba(0x000000cc))
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.volume_popup_hovered = *hovered;
                        this.update_volume_popup(cx);
                    }))
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .on_mouse_up(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                    // 百分比读数（Tauri VolumePopup 同款）
                    .child(
                        div()
                            .mb_1()
                            .text_size(gpui_kit::px(10.0))
                            .font_family("monospace")
                            .text_color(gpui_kit::rgba(0xffffffcc))
                            .child(format!(
                                "{}",
                                ((if self.muted { 0.0 } else { self.volume }) * 100.0).round()
                                    as i64
                            )),
                    )
                    .child(self.render_volume_slider(cx))
                    .into_any_element();
                gpui_kit::deferred(popup).with_priority(1)
            }))
            .into_any_element()
    }
}

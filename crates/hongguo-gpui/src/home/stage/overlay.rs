//! 舞台连播横条与进入连播态。

use super::*;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::h_flex;
use gpui_kit::{SharedString, div};

impl HomeView {
    /// 从「观看完整短剧」进入当前剧连播态（横条与取证探针共用）。
    ///
    /// 状态变化必须显式通知：wake_controls 在悬浮层已经可见时不会重绘，
    /// 只靠它会出现“点了横条没反应，直到下一次视频帧刷新”的空窗。
    pub(crate) fn enter_binge(&mut self, cx: &mut Context<Self>) {
        if let Some(now) = self.now.clone() {
            log::info!(
                "[Home] 观看完整短剧：进入连播态 {} 第 {} 集",
                now.series_id,
                now.vid_index
            );
            self.binge = Some(now.series_id);
        }
        self.wake_on_input(cx);
        cx.notify();
    }

    /// 「观看完整短剧」横条。
    ///
    /// 它必须在控制条之后挂到舞台：控制条的 `pt-10` 是透明悬停缓冲，
    /// 与横条的 `bottom: 56px` 相交；若控制条后画，它的 root down/up
    /// 拦截会把横条的点击全部吃掉（真实鼠标也点不动）。
    pub(super) fn render_binge_entry(&self, cx: &Context<Self>) -> Option<gpui_kit::AnyElement> {
        if self.mini.is_some() || self.in_binge() || !self.controls_visible() || self.now.is_none()
        {
            return None;
        }
        let theme = cx.theme();
        let now = self.now.as_ref().expect("上面已验 now 在");
        let episodes = if now.episode_count > 0 {
            format!(
                " · {}",
                crate::i18n::tf(
                    "player.totalEpisodes",
                    &[("count", &now.episode_count.to_string())],
                )
            )
        } else {
            String::new()
        };
        Some(
            h_flex()
                .id("binge-entry")
                .absolute()
                .left_3()
                .right_3()
                .bottom(gpui_kit::px(56.0))
                .justify_center()
                .gap_2()
                .rounded(theme.radius + gpui_kit::px(2.0))
                .bg(gpui_kit::rgba(0x00000099))
                .hover(|s| s.bg(gpui_kit::rgba(0x000000bf)))
                .px_3()
                .py_2p5()
                .text_sm()
                .text_color(gpui_kit::rgb(0xffffff))
                .cursor_pointer()
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.enter_binge(cx);
                    cx.stop_propagation();
                }))
                .child(div().truncate().child(SharedString::from(format!(
                    "{}{episodes}",
                    crate::i18n::t("player.watchFullSeries")
                ))))
                .child(
                    Icon::new(IconName::ChevronRight)
                        .small()
                        .text_color(gpui_kit::rgba(0xffffff99)),
                )
                .into_any_element(),
        )
    }
}

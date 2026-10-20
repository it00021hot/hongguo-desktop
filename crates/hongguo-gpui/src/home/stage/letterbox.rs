//! 系统后端的舞台黑边（letterbox）绘制。

use super::*;

use gpui_kit::div;

impl HomeView {
    /// 系统后端：把 contain 的黑边画成黑色。
    ///
    /// 透明洞的边界是**视频矩形**，不是整个舞台。视频位在 GPUI 内容之下，舞台
    /// 背景一透明，黑边就成了「洞」：Windows 的 DComp 透明窗没有窗口底色兜底，
    /// 会直接漏出后面的窗口（实测截屏里能看到别的窗口内容）；macOS 靠窗口黑底
    /// 遮住，但两侧共用这段代码之后行为一致——黑边由 GPUI 自己画，窗口底色
    /// 只作最后的兜底。
    ///
    /// 换算与 `hongguo-player-windows` 的舞台几何同参：contain 居中换算宽高，
    /// 取不到原生尺寸（metadata 未就绪）时不画；小屏是 `cover` 铺满，没有黑边。
    pub(super) fn render_stage_letterbox(&self) -> Option<gpui_kit::AnyElement> {
        if self.mini.is_some() {
            return None;
        }
        let [_, _, stage_w, stage_h] = self.stage_bounds.get()?;
        let (native_w, native_h) = self.player.as_ref()?.dims();
        if stage_w < 2.0 || stage_h < 2.0 || native_w == 0 || native_h == 0 {
            return None;
        }
        let aspect = native_w as f32 / native_h as f32;
        let (mut video_w, mut video_h) = (stage_w, stage_w / aspect);
        if video_h > stage_h {
            video_h = stage_h;
            video_w = stage_h * aspect;
        }
        let bar_y = ((stage_h - video_h) / 2.0).max(0.0);
        let bar_x = ((stage_w - video_w) / 2.0).max(0.0);
        if bar_y < 0.5 && bar_x < 0.5 {
            return None;
        }
        // 黑边往画面里压 1px：视频位是独立原生层，和最上层 GPUI 内容的像素
        // 网格在缩放后可能差半像素，边界会露出透明洞（用户实测「桌面缩放时
        // 视频边缘一条白线、像不在一个图层」）。压 1px 后边界永远被不透明
        // 黑色盖住，画面少 1px 肉眼不可见；四块都黑所以互相重叠也无妨。
        const SEAM_COVER: f32 = 1.0;
        let black = gpui_kit::rgb(0x000000);
        Some(
            div()
                .absolute()
                .size_full()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(gpui_kit::px(bar_y + SEAM_COVER))
                        .bg(black),
                )
                .child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(gpui_kit::px(bar_y + SEAM_COVER))
                        .bg(black),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .w(gpui_kit::px(bar_x + SEAM_COVER))
                        .bg(black),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(gpui_kit::px(bar_x + SEAM_COVER))
                        .bg(black),
                )
                .into_any_element(),
        )
    }
}

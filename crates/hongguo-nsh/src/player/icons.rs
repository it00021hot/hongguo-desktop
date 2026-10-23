//! 播放器图标：canvas 按 SVG path 绘制，几何与 Tauri 壳同源。
//!
//! 三套语言对齐三处来源（与 `src/features/player/components` 逐个对应）：
//! - 控制栏线框图标 = lucide（Play/Pause/SkipBack/Gauge/MonitorPlay/…），
//!   24 单位画布、1.8~2 描边、圆头圆角；
//! - 互动栏实心图标 = 抖音系自绘（实心胖星/气泡/心/转发箭头，互动栏
//!   `interaction-rail.tsx` 的同一份 path 数据）；
//! - 弹幕开关/设置 = B 站「弹」字（图标字形不是文案，不走 i18n），
//!   开关关闭加反斜杠、设置加齿轮角标（`player-icons.tsx` 同款）。
//!
//! 颜色一律收 `Fn() -> Color`：激活态（红心/黄星）随信号重画，不用重建元素。

use nsh_core::canvas::{FillRule, LineCap, LineJoin};
use nsh_core::ui::prelude::*;

/// 线框图标：24 单位画布缩放到 `size`，`width` 是 24 空间的描边宽。
fn stroked(
    size: f32,
    width: f32,
    paths: &'static [&'static str],
    color: impl Fn() -> Color + 'static,
) -> CanvasView {
    let k = size / 24.0;
    canvas(move || {
        let mut p = Painter::new()
            .scale(k, k)
            .stroke_color(color())
            .line_width(width)
            .line_cap(LineCap::Round)
            .line_join(LineJoin::Round);
        for d in paths {
            p = p.stroke_svg(d);
        }
        p.finish()
    })
    .w(px(size))
    .h(px(size))
}

/// 实心图标：`evenodd` 用于带镂空（气泡的三点）的 path。
fn filled(
    size: f32,
    paths: &'static [&'static str],
    rule: FillRule,
    color: impl Fn() -> Color + 'static,
) -> CanvasView {
    let k = size / 24.0;
    canvas(move || {
        let mut p = Painter::new().scale(k, k).fill_color(color());
        for d in paths {
            p = p.fill_path(&nsh_core::canvas::svg_path(d), rule);
        }
        p.finish()
    })
    .w(px(size))
    .h(px(size))
}

/// 实心 + 同色描边软化轮廓（抖音胖星/转发箭头的画法，同 TSX 的
/// stroke=currentColor 叠 fill）。
fn filled_soft(
    size: f32,
    paths: &'static [&'static str],
    stroke_w: f32,
    color: impl Fn() -> Color + Clone + 'static,
) -> CanvasView {
    let k = size / 24.0;
    canvas(move || {
        let c = color();
        let mut p = Painter::new()
            .scale(k, k)
            .fill_color(c)
            .stroke_color(c)
            .line_width(stroke_w)
            .line_cap(LineCap::Round)
            .line_join(LineJoin::Round);
        for d in paths {
            p = p
                .stroke_svg(d)
                .fill_path(&nsh_core::canvas::svg_path(d), FillRule::NonZero);
        }
        p.finish()
    })
    .w(px(size))
    .h(px(size))
}

// ---------------------------------------------------------------- 控制栏（lucide 线框）

pub(crate) fn play(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(size, 2.0, &["M6 3l14 9-14 9z"], color)
}

pub(crate) fn pause(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(size, 3.4, &["M8.2 5v14", "M15.8 5v14"], color)
}

pub(crate) fn skip_back(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(size, 2.0, &["M19 20L9 12l10-8z", "M5 5v14"], color)
}

pub(crate) fn skip_forward(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(size, 2.0, &["M5 4l10 8-10 8z", "M19 5v14"], color)
}

/// 倍速（Gauge：表盘 + 指针）。
pub(crate) fn gauge(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        2.0,
        &["M12 14l3.5-3.5", "M3.34 19a10 10 0 1 1 17.32 0"],
        color,
    )
}

/// 清晰度（MonitorPlay：屏幕 + 内部播放三角）。
pub(crate) fn monitor_play(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        2.0,
        &["M2 3h20v14H2Z", "M8 21h8", "M12 17v4", "M10 7l4 2.5L10 12Z"],
        color,
    )
}

/// 选集（ListVideo：画幅 + 侧列 + 底线）。
pub(crate) fn list_video(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        2.0,
        &["M3 5h11v10H3Z", "m16 7.4 6-3.4v12l-6-3.4", "M3 19h19"],
        color,
    )
}

/// 小窗（B 站画中画：屏幕 + 右下小屏 + 指入箭头，`MiniWindowIcon` 同几何）。
pub(crate) fn mini_window(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        1.8,
        &[
            "M20.6 9.4V6a2.4 2.4 0 0 0-2.4-2.4H5.8A2.4 2.4 0 0 0 3.4 6v9.6a2.4 2.4 0 0 0 2.4 2.4h3.2",
            "M14.1 12.7h4.5a2 2 0 0 1 2 2v2.7a2 2 0 0 1-2 2h-4.5a2 2 0 0 1-2-2v-2.7a2 2 0 0 1 2-2Z",
            "M7.3 8.3l3 3",
            "M10.3 8.7v2.6H7.7",
        ],
        color,
    )
}

/// 音量（Volume2：喇叭 + 双弧）。
pub(crate) fn volume(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        2.0,
        &[
            "M11 5 6 9H2v6h4l5 4V5Z",
            "M15.5 8.5a5 5 0 0 1 0 7",
            "M18.4 5.6a9 9 0 0 1 0 12.8",
        ],
        color,
    )
}

/// 静音（VolumeX：喇叭 + 叉）。
pub(crate) fn volume_x(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        2.0,
        &["M11 5 6 9H2v6h4l5 4V5Z", "m22 9-6 6", "m16 9 6 6"],
        color,
    )
}

/// 竖屏播放（RectangleVertical：竖放矩形）。
pub(crate) fn rectangle_vertical(
    size: f32,
    color: impl Fn() -> Color + Clone + 'static,
) -> CanvasView {
    stroked(
        size,
        2.0,
        &["M6 2h12a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2Z"],
        color,
    )
}

/// 退出小窗（Maximize2：对角双箭头，⤢）。
pub(crate) fn maximize(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        2.0,
        &["M15 3h6v6", "m21 3-7 7", "M9 21H3v-6", "m3 21 7-7"],
        color,
    )
}

/// 铺满档（Maximize：屏内四角外撑）。
pub(crate) fn maximize2(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        2.0,
        &[
            "M8 3H5a2 2 0 0 0-2 2v3",
            "M21 8V5a2 2 0 0 0-2-2h-3",
            "M3 16v3a2 2 0 0 0 2 2h3",
            "M16 21h3a2 2 0 0 0 2-2v-3",
        ],
        color,
    )
}

/// 退出铺满（Minimize：屏内四角收拢）。
pub(crate) fn minimize(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        2.0,
        &[
            "M8 3v3a2 2 0 0 1-2 2H3",
            "M21 8h-3a2 2 0 0 1-2-2V3",
            "M3 16h3a2 2 0 0 1 2 2v3",
            "M16 21v-3a2 2 0 0 1 2-2h3",
        ],
        color,
    )
}

/// 退出竖屏（RectangleHorizontal：横放矩形）。
pub(crate) fn rectangle_horizontal(
    size: f32,
    color: impl Fn() -> Color + Clone + 'static,
) -> CanvasView {
    stroked(
        size,
        2.0,
        &["M4 6h16a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2Z"],
        color,
    )
}

/// 置顶（Pin；pinned 时换实心）。
pub(crate) fn pin(
    size: f32,
    pinned: impl Fn() -> bool + 'static,
    color: impl Fn() -> Color + Clone + 'static,
) -> CanvasView {
    let d = "M12 17v5M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1Z";
    let k = size / 24.0;
    canvas(move || {
        let c = color();
        let mut p = Painter::new()
            .scale(k, k)
            .stroke_color(c)
            .line_width(2.0)
            .line_cap(LineCap::Round)
            .line_join(LineJoin::Round);
        let path = nsh_core::canvas::svg_path(d);
        p = if pinned() {
            p.fill_path(&path, FillRule::NonZero).stroke_path(&path)
        } else {
            p.stroke_path(&path)
        };
        p.finish()
    })
    .w(px(size))
    .h(px(size))
}

// ---------------------------------------------------------------- 互动栏（抖音系实心）

pub(crate) fn star_fill(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    filled_soft(
        size,
        &[
            "M12 17.27 18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z",
        ],
        2.2,
        color,
    )
}

pub(crate) fn comment_fill(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    filled(
        size,
        &[
            "M12 2.4C17.7 2.4 22.2 6.1 22.2 10.8C22.2 15.5 17.7 19.2 12 19.2C11.3 19.2 10.62 19.15 9.96 19.04C8.7 20 6.7 20.95 4.3 21.17C3.83 21.21 3.6 20.62 3.95 20.29C4.85 19.44 5.4 18.32 5.55 17.26C3.2 15.84 1.8 13.6 1.8 10.8C1.8 6.1 6.3 2.4 12 2.4ZM7.2 9.3A1.5 1.5 0 1 0 7.2 12.3A1.5 1.5 0 0 0 7.2 9.3ZM12 9.3A1.5 1.5 0 1 0 12 12.3A1.5 1.5 0 0 0 12 9.3ZM16.8 9.3A1.5 1.5 0 1 0 16.8 12.3A1.5 1.5 0 0 0 16.8 9.3Z",
        ],
        FillRule::EvenOdd,
        color,
    )
}

pub(crate) fn heart_fill(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    filled(
        size,
        &[
            "M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z",
        ],
        FillRule::NonZero,
        color,
    )
}

pub(crate) fn share_fill(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    filled_soft(
        size,
        &["M14 9V5l7 7-7 7v-4.1c-5 0-8.5 1.6-11 5.1 1-5 4-10 11-11z"],
        1.5,
        color,
    )
}

/// 追更（Tv：显像管电视机）。
pub(crate) fn tv(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        2.0,
        &[
            "M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z",
            "m3.3 7 8.7 5 8.7-5",
            "M12 22V12",
        ],
        color,
    )
}

// ---------------------------------------------------------------- 通用小图标

pub(crate) fn chevron_left(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(size, 2.5, &["m15 18-6-6 6-6"], color)
}

pub(crate) fn chevron_right(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(size, 2.5, &["m9 18 6-6-6-6"], color)
}

pub(crate) fn chevron_down(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(size, 2.5, &["m6 9 6 6 6-6"], color)
}

pub(crate) fn close(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(size, 2.0, &["M18 6 6 18", "m6 6 12 12"], color)
}

/// 发送（纸飞机）。
pub(crate) fn send(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(size, 2.0, &["m22 2-7 20-4-9-9-4Z", "M22 2 11 13"], color)
}

/// 加载圈（四分之三弧 + 匀速旋转，用时配 `.animate_spin()`）。
pub(crate) fn spinner(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    const QUARTER: f32 = std::f32::consts::FRAC_PI_2;
    let k = size / 24.0;
    canvas(move || {
        // 从顶部起笔顺时针扫 270°，缺口就是旋转的观感来源。
        let p = Painter::new()
            .scale(k, k)
            .stroke_color(color())
            .line_width(2.4)
            .line_cap(LineCap::Round)
            .arc(12.0, 12.0, 9.0, -QUARTER, -QUARTER + QUARTER * 3.0)
            .stroke();
        p.finish()
    })
    .w(px(size))
    .h(px(size))
}

// ---------------------------------------------------------------- 下载/隐身/窗口态

/// 下载到本地（Download：落盘托盘 + 下箭头）。
pub(crate) fn download(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        2.0,
        &[
            "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4",
            "m7 10 5 5 5-5",
            "M12 15V3",
        ],
        color,
    )
}

/// 隐身模式（Eye：眼眶 + 瞳孔）。
pub(crate) fn eye(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        2.0,
        &[
            "M2.06 12.35a1 1 0 0 1 0-.7 10.75 10.75 0 0 1 19.88 0 1 1 0 0 1 0 .7 10.75 10.75 0 0 1-19.88 0",
            "M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z",
        ],
        color,
    )
}

/// 无边框播放（网页全屏：圆角方框 + 内侧四角，`WebFullscreenIcon` 同几何）。
pub(crate) fn frameless(size: f32, color: impl Fn() -> Color + Clone + 'static) -> CanvasView {
    stroked(
        size,
        1.8,
        &[
            "M6 4.6h12a3.4 3.4 0 0 1 3.4 3.4v8a3.4 3.4 0 0 1-3.4 3.4H6a3.4 3.4 0 0 1-3.4-3.4V8A3.4 3.4 0 0 1 6 4.6Z",
            "M7.5 9.7V8.4a.9.9 0 0 1 .9-.9h1.3",
            "M16.5 9.7V8.4a.9.9 0 0 0-.9-.9h-1.3",
            "M7.5 14.3v1.3a.9.9 0 0 0 .9.9h1.3",
            "M16.5 14.3v1.3a.9.9 0 0 1-.9.9h-1.3",
        ],
        color,
    )
}

/// 全屏（四角外撑）/ 退出全屏（四角收拢），`FullscreenIcon` 两态同几何。
pub(crate) fn fullscreen(
    size: f32,
    exit: bool,
    color: impl Fn() -> Color + Clone + 'static,
) -> CanvasView {
    let paths: &'static [&'static str] = if exit {
        &[
            "M8.6 3.4v2.8a2.4 2.4 0 0 1-2.4 2.4H3.4",
            "M15.4 3.4v2.8a2.4 2.4 0 0 0 2.4 2.4h2.8",
            "M3.4 15.4h2.8a2.4 2.4 0 0 1 2.4 2.4v2.8",
            "M20.6 15.4h-2.8a2.4 2.4 0 0 0-2.4 2.4v2.8",
        ]
    } else {
        &[
            "M9.4 3.4H6A2.6 2.6 0 0 0 3.4 6v3.4",
            "M14.6 3.4H18A2.6 2.6 0 0 1 20.6 6v3.4",
            "M9.4 20.6H6A2.6 2.6 0 0 1 3.4 18v-3.4",
            "M14.6 20.6H18a2.6 2.6 0 0 0 2.6-2.6v-3.4",
        ]
    };
    stroked(size, 2.0, paths, color)
}

// ---------------------------------------------------------------- B 站「弹」字两钮

/// 弹幕开关：「弹」字 + 关闭反斜杠（`DanmakuSwitchIcon` 同款；「弹」是
/// 图标字形不是文案，不走 i18n）。
pub(crate) fn danmaku_switch(
    size: f32,
    on: impl Fn() -> bool + 'static,
    color: impl Fn() -> Color + Clone + 'static,
) -> impl IntoElement {
    let glyph = color.clone();
    let slash = color.clone();
    view()
        .w(px(size))
        .h(px(size))
        .items_center()
        .justify_center()
        .child(
            text(|| "弹".to_owned())
                .text_size(px(size * 0.72))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(glyph),
        )
        .child(If::new(
            move || !on(),
            move || {
                stroked(size * 1.2, 1.8, &["M3 3 21 21"], slash.clone())
                    .absolute()
                    .top(px(0.0))
                    .left(px(0.0))
            },
        ))
}

/// 弹幕设置：「弹」字 + 右下角齿轮角标（角标垫深色圆底，`DanmakuSettingsIcon` 同款）。
pub(crate) fn danmaku_settings(
    size: f32,
    color: impl Fn() -> Color + Clone + 'static,
) -> impl IntoElement {
    let glyph = color.clone();
    let gear = color.clone();
    view()
        .w(px(size))
        .h(px(size))
        .relative()
        .child(
            text(|| "弹".to_owned())
                .text_size(px(size * 0.72))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(glyph)
                .absolute()
                .inset(px(0.0))
                .items_center()
                .justify_center(),
        )
        .child(
            view()
                .w(px(size * 0.56))
                .h(px(size * 0.56))
                .rounded_full()
                .bg(rgba(0x000000b3))
                .items_center()
                .justify_center()
                .absolute()
                .right(px(-size * 0.22))
                .bottom(px(-size * 0.22))
                .child(stroked(
                    size * 0.44,
                    2.8,
                    &[
                        "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z",
                        "M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z",
                    ],
                    gear,
                )),
        )
}

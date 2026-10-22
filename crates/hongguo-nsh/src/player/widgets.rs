//! 播放器通用小件：可拖滑条（进度/音量/弹幕设置）、chrome 按钮、弹层遮罩、
//! 计数与时间格式。视觉口径对齐 Tauri 壳 `controls/` 下的同名件。

use nsh_core::Rect;
use nsh_core::ui::prelude::*;

/// 进度条轨迹高（静止态 2px 细条贴画面；Tauri 同款，悬停加粗不做——
/// nsh 没有逐元素 hover 事件回调，桌面端恒显 3px 折中）。
const TRACK_H: f32 = 3.0;
/// 进度条命中区高（轨迹上下各留一段好点的空白）。
const SCRUB_HIT_H: f32 = 16.0;
/// 音量竖条尺寸（Tauri `h-24 w-6`）。
const V_SLIDER_H: f32 = 96.0;
const V_SLIDER_W: f32 = 24.0;

/// 可点可拖的进度条（大屏控制栏与小窗控制条共用，`scrub-bar.tsx` 同款）。
///
/// 拖动期间进度显示跟随手势（`drag_ratio`），松手交回视频时间——网络
/// seek 有延迟，不锁显示会先弹回旧位置再跳新位置。`on_seek` 收 0..1 比例，
/// 在按下与拖动时连续回调（与 Tauri 一样边拖边 seek）。
pub(crate) fn scrub_bar(
    ratio: impl Fn() -> f32 + Clone + 'static,
    on_seek: impl Fn(f32) + Clone + 'static,
) -> impl IntoElement {
    let dragging = signal(false);
    let drag_ratio = signal(0.0_f32);
    let shown = move || {
        if dragging.get() {
            drag_ratio.get()
        } else {
            ratio().clamp(0.0, 1.0)
        }
    };
    let shown_for_knob = shown.clone();
    let track_w = signal(1.0_f32);
    let seek_at = move |x: f32| {
        let r = (x / track_w.get()).clamp(0.0, 1.0);
        drag_ratio.set(r);
        on_seek(r);
    };

    view()
        .relative()
        .w_full()
        .h(px(SCRUB_HIT_H))
        .on_layout(move |r: &Rect| {
            if r.width > 1.0 {
                track_w.set(r.width);
            }
        })
        .claim_touches(TouchClaim::on_start())
        .on_responder_grant({
            let seek_at = seek_at.clone();
            move |e: &ResponderEvent| {
                dragging.set(true);
                seek_at(e.location_x);
            }
        })
        .on_responder_move(move |e: &ResponderEvent| seek_at(e.location_x))
        .on_responder_end(move |_: &ResponderEvent| dragging.set(false))
        // 被别的视图/原生手势抢走时同样收拖动态，否则进度显示卡在手势位。
        .on_responder_terminate(move |_: &ResponderEvent| dragging.set(false))
        // 轨迹：半透明轨道 + 按比例的白色填充 + 圆点滑块。
        .child(
            view()
                .absolute()
                .left(px(0.0))
                .w_full()
                .h(px(TRACK_H))
                .top(px((SCRUB_HIT_H - TRACK_H) / 2.0))
                .rounded(px(TRACK_H / 2.0))
                .bg(rgba(0xffffff33)),
        )
        .child(
            view()
                .absolute()
                .left(px(0.0))
                .h(px(TRACK_H))
                .top(px((SCRUB_HIT_H - TRACK_H) / 2.0))
                .w(move || relative(shown()))
                .rounded(px(TRACK_H / 2.0))
                .bg(rgba(0xffffffdd)),
        )
        .child(
            view()
                .absolute()
                .h(px(10.0))
                .w(px(10.0))
                .rounded_full()
                .bg(rgb(0xffffff))
                .top(px((SCRUB_HIT_H - 10.0) / 2.0))
                .left(move || px((shown_for_knob() * track_w.get() - 5.0).max(0.0))),
        )
}

/// 竖向滑条（音量浮层用；顶部 = 1，`sliders.tsx::VerticalSlider` 同款）。
pub(crate) fn v_slider(
    value: impl Fn() -> f32 + 'static,
    on_change: impl Fn(f32) + Clone + 'static,
) -> impl IntoElement {
    slider_core(V_SLIDER_W, V_SLIDER_H, true, value, on_change)
}

/// 横向滑条（弹幕设置面板用；`sliders.tsx::DisplaySlider` 的本体，label 由
/// 调用方拼在行里）。
pub(crate) fn h_slider(
    value: impl Fn() -> f32 + 'static,
    on_change: impl Fn(f32) + Clone + 'static,
) -> impl IntoElement {
    slider_core(0.0, 4.0, false, value, on_change)
}

/// 滑条三段（轨道/已填/滑块）的自绘核心。竖条固定宽、横条占满容器；
/// 滑块挂在填充条的动端（相对长度不能与 px 混算，用父容器端部定位）。
fn slider_core(
    w: f32,
    h: f32,
    vertical: bool,
    value: impl Fn() -> f32 + 'static,
    on_change: impl Fn(f32) + Clone + 'static,
) -> impl IntoElement {
    let span = signal(1.0_f32);
    let ratio_at = move |at: f32| {
        if vertical {
            // 竖向：顶部 = 1。
            (1.0 - at / span.get()).clamp(0.0, 1.0)
        } else {
            (at / span.get()).clamp(0.0, 1.0)
        }
    };
    let axis_at = move |e: &ResponderEvent| {
        if vertical { e.location_y } else { e.location_x }
    };
    let bar = if vertical { 6.0 } else { 4.0 };
    let track = if vertical {
        view()
            .absolute()
            .left(px((V_SLIDER_W - bar) / 2.0))
            .top(px(0.0))
            .bottom(px(0.0))
            .w(px(bar))
            .rounded(px(bar / 2.0))
            .bg(rgba(0xffffff40))
    } else {
        view()
            .absolute()
            .left(px(0.0))
            .top(px((h - bar) / 2.0))
            .w_full()
            .h(px(bar))
            .rounded(px(bar / 2.0))
            .bg(rgba(0xffffff40))
    };
    // 填充条：竖向锚底、横向锚左；滑块（12px 圆点）骑在动端。
    let (fill, knob) = if vertical {
        (
            view()
                .absolute()
                .left(px((V_SLIDER_W - bar) / 2.0))
                .bottom(px(0.0))
                .w(px(bar))
                .h(move || relative(value()))
                .rounded(px(bar / 2.0))
                .bg(rgb(0xffffff)),
            view()
                .absolute()
                .top(px(-6.0))
                .left(px((bar - 12.0) / 2.0))
                .h(px(12.0))
                .w(px(12.0))
                .rounded_full()
                .bg(rgb(0xffffff)),
        )
    } else {
        (
            view()
                .absolute()
                .left(px(0.0))
                .top(px((h - bar) / 2.0))
                .h(px(bar))
                .w(move || relative(value()))
                .rounded(px(bar / 2.0))
                .bg(rgb(0xffffff)),
            view()
                .absolute()
                .right(px(-6.0))
                .top(px((bar - 12.0) / 2.0))
                .h(px(12.0))
                .w(px(12.0))
                .rounded_full()
                .bg(rgb(0xffffff)),
        )
    };
    let base = view()
        .relative()
        .claim_touches(TouchClaim::on_start())
        .on_layout(move |r: &Rect| {
            let s = if vertical { r.height } else { r.width };
            if s > 1.0 {
                span.set(s);
            }
        })
        .on_responder_grant({
            let on_change = on_change.clone();
            move |e: &ResponderEvent| on_change(ratio_at(axis_at(e)))
        })
        .on_responder_move(move |e: &ResponderEvent| on_change(ratio_at(axis_at(e))));
    if vertical {
        base.w(px(w)).h(px(h)).child(track).child(fill).child(knob)
    } else {
        base.w_full().h(px(h)).child(track).child(fill).child(knob)
    }
}

/// `75.4` s → `"1:15"`；超一小时进位到 `1:02:03`（Tauri `formatDuration` 同口径）。
pub(crate) fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0).round() as u64;
    let (h, rest) = (s / 3600, s % 3600);
    if h > 0 {
        format!("{h}:{:02}:{:02}", rest / 60, rest % 60)
    } else {
        format!("{}:{:02}", rest / 60, rest % 60)
    }
}

/// 计数格式化：抖音系「1.4万」样式（`interaction-rail.tsx::fmtCount` 同款）。
pub(crate) fn fmt_count(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}", n as f64 / 1_000_000.0)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
            + "百万"
    } else if n >= 10_000 {
        let v = n as f64 / 10_000.0;
        let digits = if n % 10_000 >= 1000 { 1 } else { 0 };
        format!("{:.*}", digits, v)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
            + "万"
    } else {
        n.to_string()
    }
}

/// 评论时间：UTC 毫秒 → 相对文案（`relativeTime` 同口径：刚刚/分钟/小时/昨天/天前）。
pub(crate) fn relative_time(create_ms: i64) -> String {
    use crate::i18n::t;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let diff = (now_ms - create_ms).max(0) / 1000;
    match diff {
        0..=59 => t("player.comments.justNow"),
        60..=3599 => t("player.comments.minutesAgo").replace("{n}", &(diff / 60).to_string()),
        3600..=86_399 => t("player.comments.hoursAgo").replace("{n}", &(diff / 3600).to_string()),
        86_400..=172_799 => t("player.comments.yesterday"),
        _ => t("player.comments.daysAgo").replace("{n}", &(diff / 86_400).to_string()),
    }
}

//! 小窗/竖屏的紧凑控制条（`mini-screen-controls.tsx` 同款三行形态）：
//! 剧名/集数/简介贴进度条上方，进度条与大屏共用，按钮行 = 播放/上下集/
//! 时间/倍速循环/静音/置顶/退出紧凑形态。竖屏形态不渲染名行（信息由
//! 沉浸信息层承担）且置顶钮不出现（Tauri 同口径）。

use nsh_core::ui::prelude::*;

use crate::i18n::t;

use super::{Stage, icons, widgets, widgets::clock};

/// 小窗倍速循环档（常用档，完整菜单回大屏）。
const MINI_RATES: [f32; 4] = [1.0, 1.5, 2.0, 3.0];

/// 紧凑控制条。`portrait` = 竖屏形态；退出按钮随形态切换。
pub(crate) fn mini_bar(s: Stage, portrait: bool) -> impl IntoElement {
    let stage = s.clone();
    let wake_stage = s.clone();
    let opacity_stage = s.clone();
    let scrub_stage = s.clone();
    view()
        .absolute()
        .left(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .flex_col()
        .gap(px(2.0))
        .px(px(8.0))
        .pt(px(6.0))
        .pb(px(6.0))
        // 底部渐隐（小窗有底板：紧凑条信息密，靠渐变保可读）。
        .bg_linear_to_t()
        .gradient_from(rgba(0x000000bf))
        .gradient_to(rgba(0x00000000))
        .on_press(move |_: &()| super::wake(&wake_stage))
        .opacity(move || {
            if opacity_stage.chrome_shown.get() {
                1.0
            } else {
                0.0_f32
            }
        })
        // 剧名/集数/简介（竖屏形态不渲染）。
        .child(If::new(
            move || !portrait,
            move || {
                let s = stage.clone();
                view()
                    .max_w(px(300.0))
                    .flex_col()
                    .child(
                        text(move || {
                            let title = s.info.title.get();
                            if title.is_empty() {
                                t("player.miniTitle")
                            } else {
                                format!("@{title}")
                            }
                        })
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(0xffffff))
                        .number_of_lines(1),
                    )
                    .child(
                        text(move || {
                            let index = s.info.vid_index.get();
                            let total = s.info.episode_count.get();
                            let mut line = if index > 0 {
                                t("player.episodeNo").replace("{index}", &index.to_string())
                            } else {
                                String::new()
                            };
                            if total > 0 {
                                line.push_str(&format!(
                                    " · {}",
                                    t("player.totalEpisodes")
                                        .replace("{count}", &total.to_string())
                                ));
                            }
                            let intro = s.info.intro.get();
                            if !intro.is_empty() {
                                line.push(' ');
                                line.push_str(&intro);
                            }
                            line
                        })
                        .text_size(px(10.0))
                        .text_color(rgba(0xffffffcc))
                        .number_of_lines(1),
                    )
            },
        ))
        // 进度条与大屏共用一份（小屏进度条一个口径）。
        .child(widgets::scrub_bar(
            move || {
                let total = scrub_stage.duration.get();
                if total <= 0.0 {
                    0.0
                } else {
                    (scrub_stage.time.get() / total).clamp(0.0, 1.0)
                }
            },
            move |ratio| super::seek_ratio(&scrub_stage, ratio),
        ))
        // 按钮行。
        .child(button_row(s.clone(), portrait))
}

/// 紧凑按钮行。闭包只捕获单个 Copy 信号或独立的 Stage 克隆。
fn button_row(s: Stage, portrait: bool) -> impl IntoElement {
    let stage = s.clone();
    let playing = s.playing;
    let time = s.time;
    let duration = s.duration;
    let rate_index = s.rate_index;
    let muted = s.muted;
    let volume = s.volume;
    view()
        .w_full()
        .h(px(28.0))
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .child(icon_button(
            "play",
            move || {
                if playing.get() {
                    icons::pause(14.0, || rgb(0xffffff))
                } else {
                    icons::play(14.0, || rgb(0xffffff))
                }
            },
            move || playing.update(|p| *p = !*p),
        ))
        .child(icon_button(
            "prev",
            move || icons::skip_back(14.0, || rgb(0xffffff)),
            {
                let s = stage.clone();
                move || super::step_episode(&s, -1)
            },
        ))
        .child(icon_button(
            "next",
            move || icons::skip_forward(14.0, || rgb(0xffffff)),
            {
                let s = stage.clone();
                move || super::step_episode(&s, 1)
            },
        ))
        .child(
            text(move || format!("{} / {}", clock(time.get()), clock(duration.get())))
                .ml(px(4.0))
                .text_size(px(10.0))
                .text_color(rgba(0xffffffd9))
                .tabular_nums(),
        )
        .child(
            view()
                .flex_1()
                .h_full()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(px(2.0))
                // 倍速：右组首位（用户口径），循环切换常用档。
                .child(icon_button(
                    "rate",
                    move || {
                        text(move || {
                            format!("{}x", MINI_RATES[current_mini_rate_at(rate_index.get())])
                        })
                        .text_size(px(10.0))
                        .text_color(rgb(0xffffff))
                        .tabular_nums()
                    },
                    move || {
                        rate_index.update(|i| {
                            // 大屏菜单的档位可能不在小窗循环表里：
                            // 从当前档的下一档继续循环。
                            let at = current_mini_rate_at(*i);
                            let want = MINI_RATES[(at + 1) % MINI_RATES.len()];
                            if let Some(idx) = super::RATES.iter().position(|r| *r == want) {
                                *i = idx;
                            }
                        });
                    },
                ))
                .child(icon_button(
                    "mute",
                    move || {
                        if muted.get() || volume.get() == 0.0 {
                            icons::volume_x(14.0, || rgb(0xffffff))
                        } else {
                            icons::volume(14.0, || rgb(0xffffff))
                        }
                    },
                    move || {
                        let next = !muted.get();
                        muted.set(next);
                        volume.set(if next { 0.0 } else { 1.0 });
                    },
                ))
                // 置顶只有小窗形态要（竖屏顶栏常驻，同款钮在顶栏——NSH
                // 壳顶栏未迁，先在小窗保留这份）。
                .child(If::new(move || !portrait, {
                    let stage = stage.clone();
                    move || {
                        let stage = stage.clone();
                        let pinned = stage.pinned;
                        icon_button(
                            "pin",
                            move || {
                                let on = pinned.get();
                                icons::pin(
                                    14.0,
                                    move || on,
                                    move || {
                                        if on { rgb(0xffffff) } else { rgba(0xffffff8c) }
                                    },
                                )
                            },
                            move || stage.toggle_pinned(),
                        )
                    }
                }))
                // ⤢ 退出当前紧凑形态（小窗 = 回大窗；竖屏 = 回横屏）。
                .child(icon_button(
                    "expand",
                    move || {
                        if portrait {
                            icons::rectangle_horizontal(14.0, || rgb(0xffffff))
                        } else {
                            icons::maximize(14.0, || rgb(0xffffff))
                        }
                    },
                    {
                        let s = stage.clone();
                        move || {
                            if portrait {
                                s.exit_portrait();
                            } else {
                                s.mini.set(false);
                            }
                        }
                    },
                )),
        )
}

/// 当前大屏档位对应的小窗循环档下标。
fn current_mini_rate_at(rate_index: usize) -> usize {
    let rate = super::RATES[rate_index.min(super::RATES.len() - 1)];
    MINI_RATES
        .iter()
        .position(|r| (*r - rate).abs() < 0.01)
        .unwrap_or(0)
}

/// 28px 紧凑方钮（半透明圆角块，悬停加深）。
fn icon_button<E: IntoElement + 'static>(
    key: &'static str,
    icon: impl Fn() -> E + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .h(px(28.0))
        .min_w(px(28.0))
        .px(px(4.0))
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .hover(|style| style.bg(rgba(0xffffff33)))
        .active_opacity(0.6)
        // 紧凑条按钮更小，扩一圈命中区（不改布局）。
        .hit_slop(EdgeInsets::all(4.0))
        .child(icon())
        .key(format!("mini-{key}"))
        .on_press(move |_: &()| on_press())
}

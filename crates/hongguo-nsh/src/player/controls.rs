//! 全量控制栏（大屏形态，`player-controls.tsx` 同款）。
//!
//! 纯悬浮：无底板，白字 + 投影压在画面上；两行 = 可拖进度条 + 按钮行。
//! 按钮行左段：播放/上下集、时间、弹幕发送框；右段：倍速、清晰度、
//! 选集、小窗、弹幕设置、弹幕开关、音量、竖屏。向上弹的浮层（倍速/
//! 清晰度/弹幕设置/音量/选集）统一锚右缘、带全屏点外关闭遮罩。
//!
//! 未迁入的 Tauri 按钮：下载（download-sheet）、隐身/无边框/全屏
//! （窗口能力，P2 随设置页一起）。

use nsh_core::ui::prelude::*;

use crate::i18n::t;

use super::{RATES, Stage, icons, picker, widgets, widgets::clock};

/// 控制栏内的弹幕发送框宽度。
const SEND_BOX_W: f32 = 200.0;
/// 向上弹浮层的统一右缘与底部锚点。
const POPUP_RIGHT: f32 = 16.0;
const POPUP_BOTTOM: f32 = 44.0;
/// 按钮格（32px 方钮，图标 16px——Tauri `size-8`/`size-4` 同格）。
const BUTTON: f32 = 32.0;

/// 控制栏（含全部浮层；调用方把它作为舞台的一个绝对定位层）。
///
/// 显隐不用响应式 `opacity`：0.1.1 上带 opacity 绑定的层命中测试会穿透
/// （点按钮落到视频），改由外层 `If` 条件挂载——代价是淡入淡出换成
/// 硬切，P2 随 nsh 升级再评估。
pub(crate) fn bottom_bar(s: Stage) -> impl IntoElement {
    let scrub_stage = s.clone();
    view()
        .absolute()
        .left(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .flex_col()
        .gap(px(6.0))
        .px(px(16.0))
        .pt(px(16.0))
        .pb(px(10.0))
        // 注意：容器**不挂** on_press(wake)——nsh 里父级 press 会截走子按钮
        // 的点击（实测点「下一集」只 wake 不切集）。按钮各自负责唤醒：
        // 播放态切换本身就会让 chrome 常显，菜单类按钮开面板同样常显。
        // 进度条。
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
        // 按钮行与浮层组。
        .child(button_row(s.clone()))
        .child(popups(s))
}

/// 按钮行：左段（播放/上下集/时间/发弹幕）+ 右段（倍速/清晰度/选集/…）。
/// 闭包只捕获单个 Copy 信号或独立的 Stage 克隆，不共享同一个句柄。
fn button_row(s: Stage) -> impl IntoElement {
    // 闭包里要整体 Stage 的动作从这里克隆（参数是引用，不能被 'static 闭包捕获）。
    let stage = s.clone();
    let playing = s.playing;
    let current_index = s.current_index;
    let episodes = s.episodes;
    let time = s.time;
    let duration = s.duration;
    let rate_index = s.rate_index;
    let definitions = s.definitions;
    let definition = s.definition;
    let muted = s.muted;
    let volume = s.volume;
    // 宽度分档（对齐 hgplayer 的逐段让位）：发送框先收缩（自身 min-w-0）；
    // <860 藏发送框、选集标签取短；<720 再藏时间与下载按钮。
    let stage_w = s.stage_w;
    let compact = move || stage_w.get() < 860.0;
    let extra_compact = move || stage_w.get() < 720.0;
    view()
        .w_full()
        .h(px(32.0))
        .flex_row()
        .items_center()
        .gap(px(2.0))
        // 播放/暂停。
        .child(icon_button(
            "play",
            move || {
                if playing.get() {
                    icons::pause(16.0, || rgb(0xffffff))
                } else {
                    icons::play(16.0, || rgb(0xffffff))
                }
            },
            move || playing.update(|p| *p = !*p),
        ))
        // 上一集（第 1 集没有上一集，置灰）。
        .child(
            view()
                .w(px(BUTTON))
                .h(px(BUTTON))
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .hover(|style| style.bg(rgba(0xffffff33)))
                .active_opacity(0.6)
                .disabled(move || current_index.get() <= 1)
                .child(icons::skip_back(16.0, || rgb(0xffffff)))
                .key("btn-prev")
                .on_press({
                    let s = stage.clone();
                    move |_: &()| super::step_episode(&s, -1)
                }),
        )
        // 下一集（表未就绪不置灰，点了由步进兜底）。
        .child(icon_button(
            "next",
            move || icons::skip_forward(16.0, || rgb(0xffffff)),
            {
                let s = s.clone();
                move || super::step_episode(&s, 1)
            },
        ))
        // 时间（等宽数字；不参与收缩——挤也不能把时间折行；极窄整段让位）。
        .child(If::new(
            move || !extra_compact(),
            move || {
                text(move || format!("{} / {}", clock(time.get()), clock(duration.get())))
                    .ml(px(6.0))
                    .flex_shrink(0.0)
                    .text_size(px(12.0))
                    .text_color(rgba(0xffffffdd))
                    .tabular_nums()
            },
        ))
        // 弹幕发送框（hgplayer 同款位置：左段时间之后；窄舞台整段让位）。
        .child(If::new(move || !compact(), {
            let s = s.clone();
            move || send_box(s.clone())
        }))
        // 右段顶到行尾。
        .child(
            view()
                .flex_1()
                .h_full()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(px(2.0))
                // 倍速。
                .child(text_button(
                    "rate",
                    move || icons::gauge(14.0, || rgb(0xffffff)),
                    move || format!("{}x", RATES[rate_index.get().min(RATES.len() - 1)]),
                    {
                        let s = stage.clone();
                        move || s.rate_menu.update(|v| *v = !*v)
                    },
                ))
                // 清晰度（无档位时不渲染菜单入口：档位列表本来就是空的，
                // 本地/未就绪的集点了也没得选——列表里说明放菜单内）。
                .child(If::new(move || !definitions.get().is_empty(), {
                    let stage = stage.clone();
                    move || {
                        text_button(
                            "definition",
                            move || icons::monitor_play(14.0, || rgb(0xffffff)),
                            move || {
                                let d = definition.get();
                                if d == 0 {
                                    t("player.definitionAuto")
                                } else {
                                    format!("{d}P")
                                }
                            },
                            {
                                let s = stage.clone();
                                move || s.def_menu.update(|v| *v = !*v)
                            },
                        )
                    }
                }))
                // 选集（沉浸流形态的数字网格入口；窄舞台标签取短）。
                .child(text_button(
                    "episodes",
                    move || icons::list_video(14.0, || rgb(0xffffff)),
                    move || {
                        let total = episodes.with(|list| list.len());
                        if compact() {
                            format!("{} {total}", t("player.episodes"))
                        } else if total > 0 {
                            format!(
                                "{} · {}",
                                t("player.episodes"),
                                t("player.totalEpisodes").replace("{count}", &total.to_string())
                            )
                        } else {
                            t("player.episodes")
                        }
                    },
                    {
                        let s = s.clone();
                        move || {
                            let next = !s.picker_open.get();
                            s.picker_open.set(next);
                            if next {
                                super::load_related(&s);
                            }
                        }
                    },
                ))
                // 下载到本地面板（Tauri 按钮同位：选集之后、小窗之前；极窄让位）。
                .child(If::new(move || !extra_compact(), {
                    let stage = stage.clone();
                    move || {
                        text_button(
                            "download",
                            move || icons::download(14.0, || rgb(0xffffff)),
                            move || t("player.download"),
                            {
                                let s = stage.clone();
                                move || s.download_open.update(|v| *v = !*v)
                            },
                        )
                    }
                }))
                // 小窗。
                .child(icon_button(
                    "mini",
                    move || icons::mini_window(16.0, || rgb(0xffffff)),
                    {
                        let s = stage.clone();
                        move || s.mini.set(true)
                    },
                ))
                // 弹幕设置。
                .child(icon_button(
                    "dm-settings",
                    move || icons::danmaku_settings(16.0, || rgb(0xffffff)),
                    {
                        let s = stage.clone();
                        move || s.dm_panel.update(|v| *v = !*v)
                    },
                ))
                // 弹幕开关。
                .child({
                    let danmaku = s.danmaku;
                    view()
                        .w(px(BUTTON))
                        .h(px(BUTTON))
                        .items_center()
                        .justify_center()
                        .rounded(px(6.0))
                        .hover(|style| style.bg(rgba(0xffffff33)))
                        .active_opacity(0.6)
                        .child(icons::danmaku_switch(
                            16.0,
                            move || danmaku.on.get(),
                            || rgb(0xffffff),
                        ))
                        .key("dm-toggle")
                        .on_press(move |_: &()| {
                            let next = !danmaku.on.get();
                            danmaku.on.set(next);
                            if !next {
                                danmaku.clear_screen();
                            }
                        })
                })
                // 音量。
                .child(icon_button(
                    "volume",
                    move || {
                        if muted.get() || volume.get() == 0.0 {
                            icons::volume_x(16.0, || rgb(0xffffff))
                        } else {
                            icons::volume(16.0, || rgb(0xffffff))
                        }
                    },
                    move || {
                        let next = !muted.get();
                        muted.set(next);
                        volume.set(if next { 0.0 } else { 1.0 });
                    },
                ))
                // 隐身模式（离窗透明+暂停，回窗续播）。
                .child(icon_button(
                    "incognito",
                    move || {
                        let on = s.incognito.get();
                        icons::eye(
                            16.0,
                            move || {
                                if on { rgb(0xffffff) } else { rgba(0xffffff8c) }
                            },
                        )
                    },
                    {
                        let s = stage.clone();
                        move || super::toggle_incognito(&s)
                    },
                ))
                // 竖屏播放（宽度按视频宽高比收窄的竖条）。
                .child(icon_button(
                    "portrait",
                    move || icons::rectangle_vertical(16.0, || rgb(0xffffff)),
                    {
                        let s = stage.clone();
                        move || s.enter_portrait()
                    },
                ))
                // 无边框播放（chrome 全藏不进 OS 全屏；hgplayer bare 同款）。
                .child(icon_button(
                    "frameless",
                    move || {
                        let on = s.frameless.get();
                        icons::frameless(
                            16.0,
                            move || {
                                if on { rgb(0xffffff) } else { rgba(0xffffffb3) }
                            },
                        )
                    },
                    {
                        let s = stage.clone();
                        move || super::toggle_frameless(&s)
                    },
                ))
                // 全屏（窗口级 OS 全屏；竖屏中让位给铺满切换，本按钮不出现）。
                .child(If::new(move || !s.portrait.get(), {
                    let s = stage.clone();
                    move || {
                        let fullscreen = s.fullscreen;
                        icon_button(
                            "fullscreen",
                            move || {
                                let exit = fullscreen.get();
                                icons::fullscreen(16.0, exit, || rgb(0xffffff))
                            },
                            {
                                let s = stage.clone();
                                move || super::toggle_fullscreen(&s)
                            },
                        )
                    }
                })),
        )
}

/// 32px 方钮：透明底白图标，悬停一层白雾，按下淡出。
fn icon_button<E: IntoElement + 'static>(
    key: &'static str,
    icon: impl Fn() -> E + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .w(px(BUTTON))
        .h(px(BUTTON))
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .hover(|style| style.bg(rgba(0xffffff33)))
        .active_opacity(0.6)
        // 32px 方钮偏小，扩一圈命中区（不改布局）。
        .hit_slop(EdgeInsets::all(4.0))
        .child(icon())
        .key(format!("btn-{key}"))
        .on_press(move |_: &()| on_press())
}

/// 带文字的 ghost 按钮（倍速/清晰度/选集，font-mono 形态）。
fn text_button<E: IntoElement + 'static>(
    key: &'static str,
    icon: impl Fn() -> E + 'static,
    label: impl Fn() -> String + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .h(px(28.0))
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .hover(|style| style.bg(rgba(0xffffff33)))
        .active_opacity(0.6)
        .child(icon())
        .child(
            text(label)
                .text_size(px(12.0))
                .text_color(rgb(0xffffff))
                .tabular_nums(),
        )
        .key(format!("chip-{key}"))
        .on_press(move |_: &()| on_press())
}

/// 弹幕发送框（圆角胶囊 + 输入 + 发送）。窄舞台整行放不下时先收缩发送框
/// （`min-w-0` 同款），按钮与时间不缩。
fn send_box(s: Stage) -> impl IntoElement {
    let draft = s.dm_draft;
    let stage = s.clone();
    view()
        .w(px(SEND_BOX_W))
        .h(px(32.0))
        .ml(px(8.0))
        .flex_shrink(1.0)
        .min_w(px(0.0))
        .overflow_hidden()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .pl(px(12.0))
        .pr(px(4.0))
        .rounded_full()
        .bg(rgba(0xffffff26))
        .child(
            input()
                .flex_1()
                .text_size(px(12.0))
                .text_color(rgb(0xffffff))
                .placeholder(move || t("player.interact.danmakuPlaceholder"))
                .placeholder_color(rgba(0xffffff66))
                .value(move || draft.get())
                .max_length(100)
                .on_change_text(move |v: &str| draft.set(v.to_owned()))
                .on_submit_editing({
                    let s = stage.clone();
                    move |_: &str| super::send_danmaku(&s)
                }),
        )
        .child({
            let s = s.clone();
            icons::send(14.0, || rgba(0xffffffcc)).on_press(move |_: &()| super::send_danmaku(&s))
        })
}

/// 浮层组：点外关闭遮罩 + 五个锚右缘的向上弹层。
fn popups(s: Stage) -> impl IntoElement {
    let s = s.clone();
    let any_open = {
        let s = s.clone();
        move || s.rate_menu.get() || s.def_menu.get() || s.dm_panel.get() || s.volume_open.get()
    };
    let dismiss_stage = s.clone();
    view()
        .child(If::new(any_open, move || {
            let s = dismiss_stage.clone();
            view()
                .absolute()
                .left(px(0.0))
                .top(px(0.0))
                .right(px(0.0))
                .bottom(px(0.0))
                .on_press(move |_: &()| s.close_popups())
        }))
        .child(rate_menu(&s))
        .child(definition_menu(&s))
        .child(danmaku_panel(&s))
        .child(volume_popup(&s))
        // 选集浮层带自己的全屏遮罩（点外部关闭），放最外层。
        .child({
            let s = s.clone();
            picker::picker(
                s.picker_open,
                s.episodes,
                s.current_index,
                s.series_id,
                s.related,
                {
                    let s = s.clone();
                    move |vid_index| super::pick_episode(&s, vid_index)
                },
                {
                    let s = s.clone();
                    move |item| {
                        s.picker_open.set(false);
                        super::switch_series(&s, &item.series_id, &item.title, &item.cover);
                    }
                },
            )
        })
}

/// 倍速菜单。
fn rate_menu(s: &Stage) -> impl IntoElement {
    let s = s.clone();
    If::new(
        move || s.rate_menu.get(),
        move || {
            let s = s.clone();
            popup(120.0, move || {
                let s = s.clone();
                view().flex_col().children(
                    RATES
                        .iter()
                        .map(|r| {
                            let s = s.clone();
                            let rate = *r;
                            let active =
                                move || RATES[s.rate_index.get().min(RATES.len() - 1)] == rate;
                            menu_item(
                                move || format!("{rate}x"),
                                active,
                                move || {
                                    if let Some(at) = RATES.iter().position(|v| *v == rate) {
                                        s.rate_index.set(at);
                                    }
                                    s.rate_menu.set(false);
                                },
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
        },
    )
}

/// 清晰度菜单：自动 + 各档位（竖屏剧的宽高是反的，只报分辨率会误导——
/// 附加 width×height 标注，Tauri 同一条说明）。
fn definition_menu(s: &Stage) -> impl IntoElement {
    let s = s.clone();
    If::new(
        move || s.def_menu.get(),
        move || {
            let s = s.clone();
            popup(160.0, move || {
                let s = s.clone();
                view()
                    .flex_col()
                    .child(menu_item(
                        move || t("player.definitionAuto"),
                        move || s.definition.get() == 0,
                        {
                            let s = s.clone();
                            move || {
                                s.definition.set(0);
                                s.def_menu.set(false);
                                super::replay_episode(&s);
                            }
                        },
                    ))
                    .children(
                        s.definitions
                            .get()
                            .iter()
                            .map(|d| {
                                let s = s.clone();
                                let value = d.value;
                                let (w, h) = (d.width, d.height);
                                menu_item(
                                    move || {
                                        if h > w {
                                            format!("{value}P {w}×{h}")
                                        } else {
                                            format!("{value}P")
                                        }
                                    },
                                    move || s.definition.get() == value,
                                    move || {
                                        s.definition.set(value);
                                        s.def_menu.set(false);
                                        super::replay_episode(&s);
                                    },
                                )
                            })
                            .collect::<Vec<_>>(),
                    )
            })
        },
    )
}

/// 弹幕设置面板：透明度 / 字号 / 密度 / 显示区域四条滑杆。
fn danmaku_panel(s: &Stage) -> impl IntoElement {
    let s = s.clone();
    If::new(
        move || s.dm_panel.get(),
        move || {
            let s = s.clone();
            popup(240.0, move || {
                let s = s.clone();
                view()
                    .flex_col()
                    .gap(px(14.0))
                    .child(slider_row(
                        t("player.danmakuOpacity"),
                        move || s.danmaku.opacity.get(),
                        move |v| s.danmaku.opacity.set(v),
                    ))
                    .child(slider_row(
                        t("player.danmakuFontSize"),
                        move || s.danmaku.font_scale.get(),
                        move |v| s.danmaku.font_scale.set(v),
                    ))
                    .child(slider_row(
                        t("player.danmakuDensity"),
                        move || s.danmaku.density.get(),
                        move |v| s.danmaku.density.set(v),
                    ))
                    .child(slider_row(
                        t("player.danmakuArea"),
                        move || s.danmaku.area.get(),
                        move |v| s.danmaku.area.set(v),
                    ))
            })
        },
    )
}

/// 弹幕设置的一行：label + 百分比 + 横滑条。
fn slider_row(
    label: String,
    value: impl Fn() -> f32 + Clone + 'static,
    on_change: impl Fn(f32) + Clone + 'static,
) -> impl IntoElement {
    view()
        .w_full()
        .flex_col()
        .gap(px(6.0))
        .child(
            view()
                .w_full()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    text(move || label.clone())
                        .text_size(px(12.0))
                        .text_color(rgba(0xffffffdd)),
                )
                .child({
                    let value = value.clone();
                    text(move || format!("{:.0}%", value() * 100.0))
                        .text_size(px(11.0))
                        .text_color(rgba(0xffffff99))
                        .tabular_nums()
                }),
        )
        .child(widgets::h_slider(value, on_change))
}

/// 音量按钮浮层：百分比 + 竖条（锚在音量钮上方）。
fn volume_popup(s: &Stage) -> impl IntoElement {
    let s = s.clone();
    If::new(
        move || s.volume_open.get(),
        move || {
            let s = s.clone();
            view()
                .absolute()
                .right(px(POPUP_RIGHT + 160.0))
                .bottom(px(POPUP_BOTTOM + 32.0))
                .w(px(48.0))
                .flex_col()
                .items_center()
                .gap(px(4.0))
                .px(px(12.0))
                .pt(px(4.0))
                .pb(px(12.0))
                .rounded(px(8.0))
                .bg(rgba(0x000000cc))
                .child(
                    text(move || format!("{:.0}", s.volume.get() * 100.0))
                        .text_size(px(10.0))
                        .text_color(rgba(0xffffffdd))
                        .tabular_nums(),
                )
                .child(widgets::v_slider(move || s.volume.get(), {
                    let s = s.clone();
                    move |v| {
                        s.volume.set(v);
                        s.muted.set(v == 0.0);
                    }
                }))
        },
    )
}

/// 向上弹浮层的容器（锚右缘、深色圆角板）。
fn popup<E: IntoElement + 'static>(w: f32, content: impl Fn() -> E + 'static) -> impl IntoElement {
    view()
        .absolute()
        .right(px(POPUP_RIGHT))
        .bottom(px(POPUP_BOTTOM))
        .w(px(w))
        .rounded(px(10.0))
        .bg(rgba(0x141414f0))
        .p(px(8.0))
        .child(content())
}

/// 菜单条目：左文案、选中 ✓。
fn menu_item(
    label: impl Fn() -> String + 'static,
    active: impl Fn() -> bool + 'static,
    on_pick: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .w_full()
        .h(px(32.0))
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(10.0))
        .rounded(px(6.0))
        .hover(|style| style.bg(rgba(0xffffff1a)))
        .child(
            text(label)
                .text_size(px(13.0))
                .text_color(rgb(0xffffff))
                .tabular_nums(),
        )
        .child(If::new(active, move || {
            text(move || "✓".to_owned())
                .text_size(px(12.0))
                .text_color(rgb(0xef4444))
        }))
        .on_press(move |_: &()| on_pick())
}

//! 选集浮层（对齐 hgplayer / Tauri `episode-picker.tsx`：「选集 | 系列剧」
//! 双 tab，贴控制栏按钮向上弹）。
//!
//! 选集 tab = 范围段（30 个一段）+ 8 列数字网格，当前集红底高亮，
//! **选中不关浮层**（hgplayer 同款连选连跳），Esc/收起/点外部才关。
//! 系列剧 tab = 同系列各季卡片，点击切季并锁定连播；未上线季给「预告」
//! 徽标（预约动作随 P2 账号域一起接）。
//!
//! 响应式口径：范围段/网格/系列卡全部走 `Each` 的条目闭包重算（信号
//! 变化 → 条目集重建）；`Stored` 不订阅，人工翻段必须用 `Signal`。

use nsh_core::ui::prelude::*;

use hongguo_api::detail::RelatedItem;
use hongguo_common::model::Episode;

use crate::i18n::t;
use crate::supply;

use super::icons;

/// 每个范围段的集数（hgplayer 同款 30 个一段）。
const GROUP_SIZE: u32 = 30;
/// 浮层尺寸（Tauri：460 宽 / 最高 62vh）。
const SHEET_W: f32 = 460.0;
const SHEET_MAX_H: f32 = 560.0;
/// 每集格子尺寸。
const CELL_W: f32 = 46.0;
const CELL_H: f32 = 34.0;

/// 选集浮层。`current` 是当前集的 vid_index（1 基）；`on_pick` 收 vid_index。
pub(crate) fn picker(
    open: Signal<bool>,
    episodes: Signal<Vec<Episode>>,
    current: Signal<u32>,
    series_id: Signal<String>,
    related: Signal<Vec<RelatedItem>>,
    on_pick: impl Fn(u32) + Clone + 'static,
    on_switch_series: impl Fn(RelatedItem) + Clone + 'static,
) -> impl IntoElement {
    let tab = signal(0_u8);
    // 手动翻到的范围段：记下翻页时的当前集——当前集一变（连播/点选）
    // 覆盖作废，自动跟回当前集所在段。
    let group_override: Signal<Option<(u32, u32)>> = signal(None);

    view()
        .absolute()
        .left(px(0.0))
        .top(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        // 点外部关闭（遮罩在浮层内容之下，点内容不会落到这里）。
        .on_press(move |_: &()| open.set(false))
        .child(If::new(
            move || open.get(),
            move || {
                // Fn 闭包里内层的 move 闭包各自吃一份独立克隆。
                let on_pick = on_pick.clone();
                let on_switch_series = on_switch_series.clone();
                view()
                    .absolute()
                    .right(px(12.0))
                    .bottom(px(56.0))
                    .w(px(SHEET_W))
                    .max_h(px(SHEET_MAX_H))
                    .flex_col()
                    .rounded(px(12.0))
                    .bg(rgba(0x171717f2))
                    .p(px(16.0))
                    .on_press(move |_: &()| {}) // 吞掉面板内点击，不落到遮罩上
                    // 头部：双 tab + 全N集 + 收起。
                    .child(
                        view()
                            .w_full()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(16.0))
                                    .child(tab_head(tab, 0, t("player.episodes")))
                                    .child(If::new(
                                        move || !related.get().is_empty(),
                                        move || {
                                            tab_head(
                                                tab,
                                                1,
                                                format!(
                                                    "{} · {}",
                                                    t("player.seriesList"),
                                                    related.get().len()
                                                ),
                                            )
                                        },
                                    )),
                            )
                            .child(
                                view()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        text(move || {
                                            let total = episodes.with(|list| list.len());
                                            if total > 0 {
                                                {
                                                    t("player.totalEpisodes")
                                                        .replace("{count}", &total.to_string())
                                                }
                                            } else {
                                                Default::default()
                                            }
                                        })
                                        .text_size(px(12.0))
                                        .text_color(rgba(0xffffff99)),
                                    )
                                    .child(
                                        icons::chevron_down(16.0, || rgba(0xffffff99))
                                            .on_press(move |_: &()| open.set(false)),
                                    ),
                            ),
                    )
                    .child(
                        scroll()
                            .flex_1()
                            .w_full()
                            .child(If::new(
                                move || tab.get() == 0,
                                move || {
                                    grid_tab(episodes, current, group_override, on_pick.clone())
                                },
                            ))
                            .child(If::new(
                                move || tab.get() == 1,
                                move || {
                                    series_tab(
                                        series_id.get(),
                                        related.get(),
                                        on_switch_series.clone(),
                                    )
                                },
                            )),
                    )
            },
        ))
}

/// 头部的一个 tab（选中 = 白字 + 红色下划线，hgplayer 同款）。
fn tab_head(tab: Signal<u8>, at: u8, label: String) -> impl IntoElement {
    let active = move || tab.get() == at;
    view()
        .flex_col()
        .items_center()
        .child(
            text(move || label.clone())
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(move || {
                    if active() {
                        rgb(0xffffff)
                    } else {
                        rgba(0xffffff66)
                    }
                })
                .pb(px(6.0)),
        )
        .child(view().w(px(24.0)).h(px(2.0)).rounded(px(1.0)).bg(move || {
            if active() {
                AdaptiveColor::from(rgb(0xef4444))
            } else {
                AdaptiveColor::TRANSPARENT
            }
        }))
        .on_press(move |_: &()| tab.set(at))
}

/// 选集 tab：范围段行 + 8 列网格。
fn grid_tab(
    episodes: Signal<Vec<Episode>>,
    current: Signal<u32>,
    group_override: Signal<Option<(u32, u32)>>,
    on_pick: impl Fn(u32) + Clone + 'static,
) -> impl IntoElement {
    // 当前段：手动覆盖只在当前集没变时生效（连播/点选即跟回所在段）。
    let active_group = computed(move || {
        let total = episodes.with(|list| list.len() as u32);
        let groups = total.div_ceil(GROUP_SIZE).max(1);
        let follow = (current.get().saturating_sub(1)) / GROUP_SIZE;
        match group_override.get() {
            Some((group, at)) if at == current.get() => group.min(groups - 1),
            _ => follow.min(groups - 1),
        }
    });
    view()
        .w_full()
        .flex_col()
        .gap(px(10.0))
        // 范围段（1-30 / 31-60 / …）。
        .child(Each::new(
            move || {
                let total = episodes.with(|list| list.len() as u32);
                (0..total.div_ceil(GROUP_SIZE).max(1)).collect::<Vec<u32>>()
            },
            |g: &u32| *g,
            move |g: ReadSignal<u32>, _| {
                let total = episodes.with(|list| list.len() as u32);
                let g_at = g.get();
                let active = move || active_group.get() == g_at;
                text(format!(
                    "{}-{}",
                    g_at * GROUP_SIZE + 1,
                    ((g_at + 1) * GROUP_SIZE).min(total)
                ))
                .text_size(px(13.0))
                .tabular_nums()
                .text_color(move || {
                    if active() {
                        rgb(0xffffff)
                    } else {
                        rgba(0xffffff66)
                    }
                })
                .font_weight(if active() {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .on_press(move |_: &()| {
                    group_override.set(Some((g_at, current.get())));
                })
            },
        ))
        // 8 列数字网格（段一变条目集整体重建）。
        .child(Each::new(
            move || {
                let total = episodes.with(|list| list.len() as u32);
                let g = active_group.get();
                let start = g * GROUP_SIZE + 1;
                let end = ((g + 1) * GROUP_SIZE).min(total);
                (start..=end).collect::<Vec<u32>>()
            },
            |vid_index: &u32| *vid_index,
            move |vid_index: ReadSignal<u32>, _| cell(current, vid_index, on_pick.clone()),
        ))
}

/// 一集格子：当前集红底白字，其余暗底；选中不关浮层（连选连跳）。
fn cell(
    current: Signal<u32>,
    vid_index: ReadSignal<u32>,
    on_pick: impl Fn(u32) + 'static,
) -> impl IntoElement {
    let active = move || current.get() == vid_index.get();
    view()
        .w(px(CELL_W))
        .h(px(CELL_H))
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .bg(move || {
            if active() {
                rgb(0xef4444)
            } else {
                rgba(0xffffff1a)
            }
        })
        .child(
            text(move || vid_index.get().to_string())
                .text_size(px(13.0))
                .tabular_nums()
                .text_color(move || {
                    if active() {
                        rgb(0xffffff)
                    } else {
                        rgba(0xffffffcc)
                    }
                }),
        )
        .on_press(move |_: &()| on_pick(vid_index.get()))
}

/// 系列剧 tab：各季卡片（当前季红框「播放中」，其余可点切季）。
fn series_tab(
    series_id: String,
    related: Vec<RelatedItem>,
    on_switch: impl Fn(RelatedItem) + Clone + 'static,
) -> impl IntoElement {
    view().w_full().flex_col().gap(px(8.0)).children(
        related
            .into_iter()
            .map(|item| card(series_id.clone(), item, on_switch.clone()))
            .collect::<Vec<_>>(),
    )
}

fn card(
    series_id: String,
    item: RelatedItem,
    on_switch: impl Fn(RelatedItem) + Clone + 'static,
) -> impl IntoElement {
    // 当前剧 = series_id 命中。plan/v 不回正在播的这部，调用方把档案
    // 合成的当前季补进列表头部（Tauri 同款），红框就落在它身上。
    // 卡片数据是静态的：展示串在构建期算好，闭包只捕获成品。
    let current = item.series_id == series_id;
    let unreleased = item.episode_cnt == 0;
    let cover = item.cover.clone();
    let title = item.title.clone();
    let mut line = item.tag.clone();
    if item.episode_cnt > 0 {
        if !line.is_empty() {
            line.push_str(" · ");
        }
        line.push_str(&t("detail.episodesCount").replace("{count}", &item.episode_cnt.to_string()));
    }
    // 「播放」胶囊与整卡的回调各自持一份独立克隆。
    let pill_item = item.clone();
    let pill_switch = on_switch.clone();
    view()
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .p(px(10.0))
        .rounded(px(8.0))
        .bg(rgba(0xffffff0d))
        .border_1()
        .border_color(move || {
            if current {
                AdaptiveColor::from(rgba(0xef4444cc))
            } else {
                AdaptiveColor::TRANSPARENT
            }
        })
        .child(
            image(move || supply::cover_url(&cover))
                .w(px(40.0))
                .h(px(56.0))
                .rounded(px(6.0))
                .resize_mode(ResizeMode::Cover),
        )
        .child(
            view()
                .flex_1()
                .flex_col()
                .gap(px(2.0))
                .child(
                    text(move || title.clone())
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(0xffffff))
                        .number_of_lines(1),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(If::new(
                            move || unreleased,
                            move || {
                                view()
                                    .px(px(4.0))
                                    .h(px(16.0))
                                    .items_center()
                                    .rounded(px(3.0))
                                    .bg(rgba(0xef4444e6))
                                    .child(
                                        text(move || t("player.comingSoonBadge"))
                                            .text_size(px(10.0))
                                            .text_color(rgb(0xffffff)),
                                    )
                            },
                        ))
                        .child(
                            text(move || line.clone())
                                .text_size(px(11.0))
                                .text_color(rgba(0xffffff80))
                                .number_of_lines(1),
                        ),
                ),
        )
        .child(If::new(
            move || current,
            move || {
                text(move || t("player.nowPlaying"))
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(0xef4444))
            },
        ))
        .child(If::new(
            move || !current && !unreleased,
            move || {
                let item = pill_item.clone();
                let on_switch = pill_switch.clone();
                text(move || t("player.play"))
                    .text_size(px(12.0))
                    .text_color(rgba(0xffffffcc))
                    .px(px(12.0))
                    .h(px(28.0))
                    .items_center()
                    .rounded_full()
                    .border_1()
                    .border_color(rgba(0xffffff59))
                    .on_press(move |_: &()| on_switch(item.clone()))
            },
        ))
        // 当前季与未上线的卡不可点；其余整卡可点。
        .on_press(move |_: &()| {
            if !current && !unreleased {
                on_switch(item.clone());
            }
        })
}

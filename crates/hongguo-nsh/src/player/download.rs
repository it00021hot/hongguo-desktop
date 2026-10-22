//! 播放器里的「下载到本地」面板（Tauri `download-sheet.tsx` 同流）：
//! 选集多选（每次打开都回到「当前这一集」）+ 全选/清空/后 30 集快选 +
//! 提交入队。任务进度不在播放页显示——提交成功即关面板（Tauri 跳任务页，
//! 本壳任务页 P2 迁入）。

use nsh_core::ui::prelude::*;

use hongguo_common::model::Episode;
use hongguo_service::app_state::AppState;

use crate::i18n::t;

use super::icons;

/// 「后 30 集」快选的集数。
const LAST_N: u32 = 30;

/// 每集格子（与选集浮层同款尺寸，多选态）。
const CELL_W: f32 = 46.0;
const CELL_H: f32 = 34.0;

/// 下载面板：全屏遮罩（点外关）+ 底部面板。勾选状态在这里持有；
/// `series_id` 是提交对象，`default_selected` 是当前正在播的集。
/// 整棵按 `open` 条件挂载——遮罩常驻会把整窗点击全部吞掉。
pub(crate) fn sheet(
    open: Signal<bool>,
    episodes: Signal<Vec<Episode>>,
    default_selected: Signal<u32>,
    series_id: Signal<String>,
) -> impl IntoElement {
    let selected: Signal<Vec<u32>> = signal_with(Vec::new(), SignalEquals::Never);
    // 每次打开都重置勾选为当前集（Tauri handleOpen 同款）。
    effect(move || {
        if open.get() {
            let current = default_selected.get_untracked();
            selected.update(|list| {
                list.clear();
                if current > 0 {
                    list.push(current);
                }
            });
        }
    });
    If::new(
        move || open.get(),
        move || {
            let selected = selected;
            view()
                .absolute()
                .left(px(0.0))
                .top(px(0.0))
                .right(px(0.0))
                .bottom(px(0.0))
                .on_press(move |_: &()| open.set(false))
                .child(
                    view()
                        .absolute()
                        .left(px(0.0))
                        .right(px(0.0))
                        .bottom(px(0.0))
                        .h(px(420.0))
                        .flex_col()
                        .rounded(px(12.0))
                        .bg(rgba(0x171717f2))
                        .p(px(16.0))
                        .on_press(move |_: &()| {}) // 吞面板内点击
                        .child(header(open))
                        .child(quick_row(episodes, selected))
                        .child(
                            // 固定高：max_h 容器里 flex_1 没有可分高度，网格会塌。
                            // 8 列网格（CSS grid；flex_wrap 的宽度解析在
                            // absolute 链里不可靠）。
                            scroll().h(px(280.0)).w_full().child(
                                view()
                                    .w_full()
                                    .grid()
                                    .grid_cols(8)
                                    .gap(px(8.0))
                                    .child(Each::new(
                                        move || episodes.with(|list| list.clone()),
                                        |e: &Episode| e.vid_index,
                                        move |item: ReadSignal<Episode>, _| cell(selected, item),
                                    )),
                            ),
                        )
                        .child(footer(open, series_id, selected)),
                )
        },
    )
}

/// 标题行：标题 + 关闭。
fn header(open: Signal<bool>) -> impl IntoElement {
    view()
        .w_full()
        .flex_row()
        .items_center()
        .justify_between()
        .child(
            text(move || t("player.downloadToLocal"))
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0xffffff)),
        )
        .child(icons::close(16.0, || rgba(0xffffff99)).on_press(move |_: &()| open.set(false)))
}

/// 快选行：全选 / 清空 / 后 30 集。
fn quick_row(episodes: Signal<Vec<Episode>>, selected: Signal<Vec<u32>>) -> impl IntoElement {
    view()
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .py(px(10.0))
        .child(quick_button("download.selectAll", {
            move || {
                let all =
                    episodes.with(|list| list.iter().map(|e| e.vid_index).collect::<Vec<_>>());
                selected.set(all);
            }
        }))
        .child(quick_button("download.clear", {
            move || selected.set(Vec::new())
        }))
        .child(quick_button("download.last30", {
            move || {
                let total = episodes.with(|list| list.len() as u32);
                let start = total.saturating_sub(LAST_N).max(1);
                let tail = episodes.with(|list| {
                    list.iter()
                        .filter(|e| e.vid_index >= start)
                        .map(|e| e.vid_index)
                        .collect::<Vec<_>>()
                });
                selected.set(tail);
            }
        }))
}

/// 快选按钮。
fn quick_button(key: &'static str, action: impl Fn() + 'static) -> impl IntoElement {
    text(move || t(key))
        .text_size(px(12.0))
        .text_color(rgba(0xffffffcc))
        .px(px(10.0))
        .h(px(26.0))
        .items_center()
        .rounded(px(6.0))
        .bg(rgba(0xffffff1a))
        .hover(|style| style.bg(rgba(0xffffff33)))
        .on_press(move |_: &()| action())
}

/// 一集格子（多选；点选/再点取消）。
fn cell(selected: Signal<Vec<u32>>, item: ReadSignal<Episode>) -> impl IntoElement {
    let vid_index = item.with(|e| e.vid_index);
    let active = move || selected.with(|list| list.contains(&vid_index));
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
            text(move || vid_index.to_string())
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
        .on_press(move |_: &()| {
            selected.update(|list| {
                if list.contains(&vid_index) {
                    list.retain(|v| *v != vid_index);
                } else {
                    list.push(vid_index);
                    list.sort_unstable();
                }
            });
        })
}

/// 底部：已选 N 集 + 取消 / 开始下载。
fn footer(
    open: Signal<bool>,
    series_id: Signal<String>,
    selected: Signal<Vec<u32>>,
) -> impl IntoElement {
    view()
        .w_full()
        .flex_row()
        .items_center()
        .justify_between()
        .pt(px(10.0))
        .child(
            text(move || {
                t("download.selected")
                    .replace("{count}", &selected.with(|list| list.len()).to_string())
            })
            .text_size(px(12.0))
            .text_color(rgba(0xffffff99))
            .tabular_nums(),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    text(move || t("common.cancel"))
                        .text_size(px(13.0))
                        .text_color(rgba(0xffffffcc))
                        .px(px(14.0))
                        .h(px(30.0))
                        .items_center()
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(rgba(0xffffff40))
                        .on_press(move |_: &()| open.set(false)),
                )
                .child(submit_button(open, series_id, selected)),
        )
}

/// 提交按钮：入队下载任务（与 Tauri `download_batch` 同一 service 调用
/// `enqueue_batch`，同步排队动作直接调），成功 toast + 关面板。
fn submit_button(
    open: Signal<bool>,
    series_id: Signal<String>,
    selected: Signal<Vec<u32>>,
) -> impl IntoElement {
    view()
        .h(px(30.0))
        .px(px(14.0))
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .rounded(px(6.0))
        .bg(rgb(0xef4444))
        .hover(|style| style.bg(rgba(0xef4444cc)))
        .child(icons::download(14.0, || rgb(0xffffff)))
        .child(
            text(move || t("download.submit"))
                .text_size(px(13.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(0xffffff)),
        )
        .on_press(move |_: &()| {
            let vids = selected.with(|list| list.clone());
            if vids.is_empty() {
                return;
            }
            // vid_index 集合交给 service（它自己解析 vid 与标题入队）。
            let state: AppState = crate::state::state();
            let series = series_id.get();
            match hongguo_service::download_service::actions::enqueue_batch(&state, &series, &vids)
            {
                Ok(queued) => {
                    toast(
                        t("download.submitting").replace("{count}", &queued.to_string()),
                        ToastDuration::Short,
                    );
                    open.set(false);
                }
                Err(e) => log::warn!("[Download] 入队失败: {e}"),
            }
        })
}

//! 观看历史页（Tauri `history-page` / GPUI `pages/history` 的 NSH 对齐）。
//!
//! **双模式**（issue #10 同款）：登录 = 云端观看记录（跨设备）；未登录 =
//! 本地观看记录（播放时已落库），顶部给本地模式提示行。行式列表：封面 +
//! 标题 + 进度行 + 相对时间，整行点击续播；「全部/未看完/已看完」筛选 +
//! 标题搜索。已看完的剧不续播——按列表顺序跳下一部未看完的（末尾回绕，
//! GPUI/Tauri 同口径）。批量删除留 P3（README 已注明）。

use nsh_core::ui::prelude::*;

use hongguo_api::history::WatchHistoryItem;

use crate::i18n::t;
use crate::nav::{Nav, Page};
use crate::pages::common::{PAGE_PAD, Phase, empty_body, failed_body, loading_body, pill};
use crate::player::widgets::relative_time;

/// 筛选 tab（Tauri 全部/未看完/已看完）。
const TABS: [(&str, &str, u8); 3] = [
    ("history.tabAll", "hist-tab-all", 0),
    ("history.tabUnfinished", "hist-tab-unfinished", 1),
    ("history.tabFinished", "hist-tab-finished", 2),
];

#[component]
pub fn HistoryPage() -> impl IntoElement {
    let items: Signal<Vec<WatchHistoryItem>> = signal_with(Vec::new(), SignalEquals::Never);
    let phase = signal(Phase::Loading);
    let tab = signal(0u8);
    let search = signal(String::new());
    let loading = stored(false);
    let nav = Nav::from_context();
    let logged = crate::account::logged_in();

    // 挂载拉一次；登录态翻沿（登录成功 / 退出）在云端↔本地双模式间重拉。
    {
        effect(move || {
            logged.get();
            let (items, phase, loading) = (items, phase, loading);
            spawn(async move {
                load(items, phase, loading).await;
            });
        });
    }

    view()
        .flex_1()
        .w_full()
        .h_full()
        .overflow_hidden()
        .flex_col()
        .px(px(PAGE_PAD))
        .pt(px(PAGE_PAD))
        .gap(px(12.0))
        // 工具行：筛选 tab + 搜索框。
        .child(
            view()
                .w_full()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .children(TABS.map(|(key, id, value)| {
                    let active = move || tab.get() == value;
                    pill(
                        id,
                        move || t(key),
                        move || {
                            if active() {
                                theme().primary
                            } else {
                                theme().muted
                            }
                        },
                        move || {
                            if active() {
                                theme().primary_foreground
                            } else {
                                theme().muted_foreground
                            }
                        },
                        move || tab.set(value),
                    )
                }))
                // 占位 flex：把搜索框推到行尾。
                .child(view().flex_1())
                .child(search_box(search)),
        )
        // 未登录：本地模式提示行（播放记录只在本机）。
        .child(If::new(
            move || !logged.get(),
            move || {
                text(move || t("history.localModeHint"))
                    .text_size(px(12.0))
                    .text_color(theme().muted_foreground)
            },
        ))
        // 主体：阶段态 / 列表。
        .child(If::new(move || phase.get() == Phase::Loading, loading_body))
        .child(If::new(
            move || matches!(phase.get(), Phase::Failed(_)),
            move || {
                let Phase::Failed(message) = phase.get() else {
                    return view().into_element();
                };
                let message = signal(message);
                failed_body(message, move || {
                    let (items, phase, loading) = (items, phase, loading);
                    spawn(async move {
                        load(items, phase, loading).await;
                    });
                })
                .into_element()
            },
        ))
        .child(If::new(
            move || phase.get() == Phase::Loaded,
            move || {
                if items.get().is_empty() {
                    return empty_body("history.empty", Some("history.emptyHint")).into_element();
                }
                let rows = visible(&items, tab, search);
                if rows.is_empty() {
                    let key = match tab.get() {
                        1 => "history.noMatchUnfinished",
                        2 => "history.noMatchFinished",
                        _ => "history.empty",
                    };
                    return empty_body(key, None).into_element();
                }
                scroll()
                    .flex_1()
                    .w_full()
                    .child(Each::new(
                        move || visible(&items, tab, search),
                        |item: &WatchHistoryItem| item.series_id.clone(),
                        move |item: ReadSignal<WatchHistoryItem>, _| row(nav, item, items),
                    ))
                    .into_element()
            },
        ))
}

/// 筛选（tab + 标题/seriesId 包含，Tauri `matchListQuery` 同口径）。
fn visible(
    items: &Signal<Vec<WatchHistoryItem>>,
    tab: Signal<u8>,
    search: Signal<String>,
) -> Vec<WatchHistoryItem> {
    let tab = tab.get();
    let q = search.get().trim().to_lowercase();
    items
        .get()
        .into_iter()
        .filter(|item| {
            let finished = item.is_finished();
            match tab {
                1 if finished => return false,
                2 if !finished => return false,
                _ => {}
            }
            q.is_empty()
                || item.title.to_lowercase().contains(&q)
                || item.series_id.to_lowercase().contains(&q)
        })
        .collect()
}

/// 一行历史：封面 + 标题 + 进度行；整行点击续播。
fn row(
    nav: Nav,
    item: ReadSignal<WatchHistoryItem>,
    all: Signal<Vec<WatchHistoryItem>>,
) -> impl IntoElement {
    let key = format!("hist-{}", item.get().series_id);
    view()
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .p(px(8.0))
        .rounded(px(10.0))
        .bg(theme().group_box)
        .child(
            // 封面 16:9 小图（封面缺省给纯色底）。
            view()
                .w(px(96.0))
                .h(px(54.0))
                .rounded(px(8.0))
                .overflow_hidden()
                .bg(theme().muted)
                .child(If::new(
                    move || !item.get().cover.is_empty(),
                    move || {
                        image(move || crate::supply::cover_url(&item.get().cover))
                            .w_full()
                            .h_full()
                            .resize_mode(ResizeMode::Cover)
                    },
                )),
        )
        .child(
            view()
                .flex_1()
                .overflow_hidden()
                .flex_col()
                .gap(px(4.0))
                .child(
                    text(move || {
                        let title = item.get().title;
                        if title.is_empty() {
                            t("history.unknownTitle")
                        } else {
                            title
                        }
                    })
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme().foreground),
                )
                .child(
                    text(move || progress_line(&item.get()))
                        .text_size(px(12.0))
                        .text_color(theme().muted_foreground),
                ),
        )
        .child(
            text(move || t("history.continue"))
                .text_size(px(12.0))
                .text_color(theme().primary_foreground)
                .px(px(12.0))
                .py(px(5.0))
                .rounded(px(999.0))
                .bg(theme().primary),
        )
        .key(key)
        .on_press(move |_: &()| open_item(&nav, &all.get(), &item.get()))
}

/// 进度行：`看到第N集/共M集 相对时间`（条目没给总集数时只给相对时间）。
fn progress_line(item: &WatchHistoryItem) -> String {
    let time = relative_time(item.updated_at_ms);
    if item.episode_cnt > 0 {
        t("history.watching")
            .replace("{index}", &item.vidx_1based().to_string())
            .replace("{total}", &item.episode_cnt.to_string())
            .replace("{time}", &time)
    } else {
        time
    }
}

/// 续播：已看完的剧跳到下一部未看完的（末尾回绕）；剩余 <2s 视为本集已
/// 看完直接给下一集 0s，绝不起播在尾部（GPUI/Tauri 同口径）。
fn open_item(nav: &Nav, items: &[WatchHistoryItem], item: &WatchHistoryItem) {
    let target = if item.is_finished() {
        let start = items
            .iter()
            .position(|row| row.series_id == item.series_id)
            .map(|index| index + 1)
            .unwrap_or(0);
        items[start..]
            .iter()
            .chain(items[..start].iter())
            .find(|row| !row.is_finished())
    } else {
        Some(item)
    };
    let Some(target) = target else {
        log::info!("[History] 已看完且无未完成项，跳过 {}", item.series_id);
        return;
    };
    let mut vid_index = target.vidx_1based() as u32;
    let mut start_secs = (target.position_ms as f64 / 1000.0).max(0.0);
    if target.duration_ms > 0 && target.duration_ms - target.position_ms <= 2000 {
        vid_index = vid_index.saturating_add(1);
        start_secs = 0.0;
    }
    if target.series_id != item.series_id {
        log::info!(
            "[History] 已看完《{}》→ 跳过，接下一部《{}》",
            item.series_id,
            target.series_id
        );
    }
    crate::play_req::request(target.series_id.clone(), vid_index, start_secs);
    nav.goto(Page::Home);
}

/// 双模式拉取：登录 = 云端首屏一页；未登录 = 本地观看记录（全量读）。
async fn load(items: Signal<Vec<WatchHistoryItem>>, phase: Signal<Phase>, loading: Stored<bool>) {
    if loading.with(|v| *v) {
        return;
    }
    loading.replace(true);
    if items.get().is_empty() {
        phase.set(Phase::Loading);
    }
    let state = crate::state::state();
    let logged = state.settings().account.is_some();
    let result: Result<Vec<WatchHistoryItem>, String> = if logged {
        let env = state.api_env();
        match background(async move { hongguo_api::history::fetch_watch_history(0, &env).await })
            .await
        {
            Ok(Ok(page)) => {
                log::info!("[History] 云端 {} 条", page.items.len());
                Ok(page.items)
            }
            Ok(Err(e)) => Err(e.to_string()),
            Err(e) => Err(e.to_string()),
        }
    } else {
        let store = state.store.clone();
        match background(async move {
            tokio::task::spawn_blocking(move || store.local_history_list())
                .await
                .map_err(|e| format!("本地历史读取任务失败: {e}"))
                .and_then(|inner| inner.map_err(|e| e.to_string()))
        })
        .await
        {
            Ok(Ok(list)) => {
                log::info!("[History] 本地 {} 条", list.len());
                Ok(list
                    .into_iter()
                    .map(|it| WatchHistoryItem {
                        series_id: it.series_id,
                        title: it.title,
                        cover: it.cover,
                        vid_index: i64::from(it.vid_index),
                        vid: it.vid,
                        position_ms: it.position_ms,
                        duration_ms: it.duration_ms,
                        episode_cnt: i64::from(it.episode_cnt),
                        updated_at_ms: it.updated_at_ms,
                    })
                    .collect())
            }
            Ok(Err(e)) => Err(e),
            Err(e) => Err(e.to_string()),
        }
    };
    loading.replace(false);
    match result {
        Ok(list) => batch(|| {
            items.set(list);
            phase.set(Phase::Loaded);
        }),
        Err(e) => {
            log::warn!("[History] 加载失败: {e}");
            if items.get().is_empty() {
                phase.set(Phase::Failed(e));
            }
        }
    }
}

/// 页内搜索框（圆角胶囊）。
fn search_box(value: Signal<String>) -> impl IntoElement {
    view()
        .w(px(220.0))
        .h(px(28.0))
        .px(px(10.0))
        .flex_row()
        .items_center()
        .rounded(px(999.0))
        .bg(theme().muted)
        .child(
            input()
                .flex_1()
                .text_size(px(12.0))
                .text_color(theme().foreground)
                .placeholder(move || t("history.searchPlaceholder"))
                .placeholder_color(crate::tokens::icon_muted())
                .value(move || value.get())
                .max_length(50)
                .on_change_text(move |v: &str| value.set(v.to_owned())),
        )
}

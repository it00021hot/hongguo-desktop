//! 收藏页（Tauri `collection-page` / GPUI `pages/collections` 的 NSH 对齐）。
//!
//! 书架列表（bookshelf/video/list）：卡片网格（封面 3:4 + 标题 + 共 N 集），
//! 「继续播放」走本地续播锚点，「✕」单条取消收藏（bookshelf update
//! operate_type=1）；左上类型筛选（全部/真人/漫剧，按条目 content_type）+
//! 标题搜索。未登录给登录墙（匿名书架只给空表，不发请求）。管理模式批量
//! 取消留 P3（README 已注明）。
//!
//! 书架条目只有 series_id：标题/集数/封面逐条经 series resolver 解析，
//! 到手一张插一张（与 GPUI 的 resolving 集合同口径）。

use std::collections::HashMap;

use nsh_core::ui::prelude::*;

use hongguo_api::interact::BookshelfEntry;

use crate::i18n::t;
use crate::nav::{Nav, Page};
use crate::pages::common::{
    PAGE_PAD, Phase, empty_body, failed_body, loading_body, login_body, pill,
};
use crate::tokens::scrim;

/// 内容类型筛选（Tauri 按 content_type：1=真人 1004=漫剧）。
const TABS: [(&str, &str, u8, i64); 3] = [
    ("collections.filterAll", "col-tab-all", 0, 0),
    ("collections.filterReal", "col-tab-real", 1, 1),
    ("collections.filterComic", "col-tab-comic", 2, 1004),
];

/// 卡面宽（Tauri/GPUI 收藏卡同档）。
const CARD_W: f32 = 172.0;
/// 封面高（3:4）。
const COVER_H: f32 = 229.0;

/// 一条书架卡面的解析结果（书架条目本身只有 series_id）。
#[derive(Debug, Clone, Default)]
struct Visual {
    title: String,
    episode_count: u32,
    cover: String,
}

#[component]
pub fn CollectionsPage() -> impl IntoElement {
    let entries: Signal<Vec<BookshelfEntry>> = signal_with(Vec::new(), SignalEquals::Never);
    let visuals: Signal<HashMap<String, Visual>> = signal_with(HashMap::new(), SignalEquals::Never);
    let phase = signal(Phase::Loading);
    let tab = signal(0u8);
    let search = signal(String::new());
    let loading = stored(false);
    let nav = Nav::from_context();
    let logged = crate::account::logged_in();

    // 挂载拉一次；登录态翻沿（登录成功 / 退出）重拉。
    {
        effect(move || {
            logged.get();
            let (entries, visuals, phase, loading) = (entries, visuals, phase, loading);
            spawn(async move {
                load(entries, visuals, phase, loading).await;
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
        // 工具行：类型筛选 + 搜索框。
        .child(
            view()
                .w_full()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .children(TABS.map(|(key, id, value, content_type)| {
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
                .child(view().flex_1())
                .child(search_box(search)),
        )
        // 主体：阶段态 / 网格。
        .child(If::new(move || phase.get() == Phase::Loading, loading_body))
        .child(If::new(
            move || phase.get() == Phase::LoginRequired,
            move || login_body("collections.loginRequired"),
        ))
        .child(If::new(
            move || matches!(phase.get(), Phase::Failed(_)),
            move || {
                let Phase::Failed(message) = phase.get() else {
                    return view().into_element();
                };
                let message = signal(message);
                failed_body(message, move || {
                    let (entries, visuals, phase, loading) = (entries, visuals, phase, loading);
                    spawn(async move {
                        load(entries, visuals, phase, loading).await;
                    });
                })
                .into_element()
            },
        ))
        .child(If::new(
            move || phase.get() == Phase::Loaded,
            move || {
                if entries.get().is_empty() {
                    return empty_body("collections.empty", Some("collections.emptyHint"))
                        .into_element();
                }
                let rows = visible(&entries, &visuals, tab, search);
                if rows.is_empty() {
                    return empty_body("collections.empty", None).into_element();
                }
                scroll()
                    .flex_1()
                    .w_full()
                    .child(
                        view()
                            .w_full()
                            .flex_row()
                            .flex_wrap()
                            .gap(px(12.0))
                            .child(Each::new(
                                move || visible(&entries, &visuals, tab, search),
                                |entry: &BookshelfEntry| entry.series_id.clone(),
                                move |entry: ReadSignal<BookshelfEntry>, _| {
                                    card(nav, entry, entries, visuals, phase, loading)
                                },
                            )),
                    )
                    .into_element()
            },
        ))
}

/// 筛选（类型 tab + 标题/seriesId 包含，Tauri `matchListQuery` 同口径）。
fn visible(
    entries: &Signal<Vec<BookshelfEntry>>,
    visuals: &Signal<HashMap<String, Visual>>,
    tab: Signal<u8>,
    search: Signal<String>,
) -> Vec<BookshelfEntry> {
    let tab = tab.get();
    let q = search.get().trim().to_lowercase();
    entries
        .get()
        .into_iter()
        .filter(|entry| {
            match tab {
                1 if entry.content_type != 1 => return false,
                2 if entry.content_type != 1004 => return false,
                _ => {}
            }
            if q.is_empty() {
                return true;
            }
            let title = visuals
                .get()
                .get(&entry.series_id)
                .map(|v| v.title.clone())
                .unwrap_or_default();
            title.to_lowercase().contains(&q) || entry.series_id.to_lowercase().contains(&q)
        })
        .collect()
}

/// 一张收藏卡：封面（右上 ✕ 取消收藏）+ 继续播放 + 标题 + 共 N 集。
fn card(
    nav: Nav,
    entry: ReadSignal<BookshelfEntry>,
    all: Signal<Vec<BookshelfEntry>>,
    visuals: Signal<HashMap<String, Visual>>,
    phase: Signal<Phase>,
    loading: Stored<bool>,
) -> impl IntoElement {
    let sid = entry.get().series_id.clone();
    let key = format!("col-{}", sid);
    let remove_sid = sid.clone();
    let open_sid = sid.clone();
    view()
        .w(px(CARD_W))
        .flex_col()
        .gap(px(6.0))
        // 封面 3:4；右上 ✕ 单条取消收藏（封面容器不收点击，✕ 自己收）。
        .child(
            view()
                .w_full()
                .h(px(COVER_H))
                .relative()
                .rounded(px(10.0))
                .overflow_hidden()
                .bg(theme().muted)
                .child(If::new(
                    move || {
                        visuals
                            .get()
                            .get(&entry.get().series_id)
                            .is_some_and(|v| !v.cover.is_empty())
                    },
                    move || {
                        image(move || {
                            let cover = visuals
                                .get()
                                .get(&entry.get().series_id)
                                .map(|v| v.cover.clone())
                                .unwrap_or_default();
                            crate::supply::cover_url(&cover)
                        })
                        .w_full()
                        .h_full()
                        .resize_mode(ResizeMode::Cover)
                    },
                ))
                .child(
                    text("✕")
                        .text_size(px(11.0))
                        .text_color(theme().foreground)
                        .w(px(22.0))
                        .h(px(22.0))
                        .items_center()
                        .justify_center()
                        .absolute()
                        .right(px(6.0))
                        .top(px(6.0))
                        .rounded(px(999.0))
                        .bg(scrim())
                        .key("col-remove")
                        .on_press(move |_: &()| {
                            remove_one(&remove_sid, all, visuals, phase, loading)
                        }),
                ),
        )
        .child(
            text(move || t("history.continue"))
                .text_size(px(12.0))
                .text_color(theme().primary_foreground)
                .w_full()
                .h(px(28.0))
                .items_center()
                .justify_center()
                .rounded(px(8.0))
                .bg(theme().primary)
                .key("col-play")
                .on_press(move |_: &()| open_series(&nav, &open_sid)),
        )
        .child(
            text(move || {
                let title = visuals
                    .get()
                    .get(&entry.get().series_id)
                    .map(|v| v.title.clone())
                    .unwrap_or_default();
                if title.is_empty() {
                    let tail: String = entry
                        .get()
                        .series_id
                        .chars()
                        .rev()
                        .take(6)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect();
                    t("collections.unresolved").replace("{id}", &tail)
                } else {
                    title
                }
            })
            .text_size(px(13.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme().foreground),
        )
        .child(
            text(move || {
                let visual = visuals.get().get(&entry.get().series_id).cloned();
                match visual {
                    Some(v) if v.episode_count > 0 => {
                        t("collections.episodeCount").replace("{n}", &v.episode_count.to_string())
                    }
                    _ => t("collections.resolvingHint"),
                }
            })
            .text_size(px(12.0))
            .text_color(theme().muted_foreground),
        )
        .key(key)
}

/// 续播一部收藏：本地播放锚点（没看过从第 1 集起）。
fn open_series(nav: &Nav, series_id: &str) {
    let state = crate::state::state();
    let (vid_index, start_secs) =
        hongguo_service::play_service::resume::resolve_entry(&state.store, series_id, &[])
            .ok()
            .unwrap_or((1, 0.0));
    crate::play_req::request(series_id.to_owned(), vid_index, start_secs);
    nav.goto(Page::Home);
}

/// 单条取消收藏（bookshelf update operate_type=1），成功后重拉列表。
fn remove_one(
    series_id: &str,
    entries: Signal<Vec<BookshelfEntry>>,
    visuals: Signal<HashMap<String, Visual>>,
    phase: Signal<Phase>,
    loading: Stored<bool>,
) {
    let state = crate::state::state();
    let sid = series_id.to_owned();
    spawn(async move {
        let env = state.api_env();
        let result =
            background(
                async move { hongguo_api::interact::collect_series(&sid, false, &env).await },
            )
            .await;
        match result {
            Ok(Ok(())) => {
                toast(t("collections.removed"), ToastDuration::Short);
                load(entries, visuals, phase, loading).await;
            }
            Ok(Err(e)) => log::warn!("[Collections] 取消收藏失败: {e}"),
            Err(e) => log::warn!("[Collections] 取消收藏任务失败: {e}"),
        }
    });
}

/// 双模式下重拉（登录态翻沿时 visuals 清空重建）。
async fn load(
    entries: Signal<Vec<BookshelfEntry>>,
    visuals: Signal<HashMap<String, Visual>>,
    phase: Signal<Phase>,
    loading: Stored<bool>,
) {
    if loading.with(|v| *v) {
        return;
    }
    loading.replace(true);
    let state = crate::state::state();
    let Some(account) = state.settings().account else {
        // 未登录：登录墙（匿名书架只给空表，不发请求）
        batch(|| {
            entries.set(Vec::new());
            visuals.set(HashMap::new());
            phase.set(Phase::LoginRequired);
        });
        loading.replace(false);
        return;
    };
    if entries.get().is_empty() {
        phase.set(Phase::Loading);
    }
    let env = state.api_env();
    let uid = account.user_id;
    let result =
        background(async move { hongguo_api::interact::fetch_bookshelf(&uid, &env).await }).await;
    loading.replace(false);
    match result {
        Ok(Ok(list)) => {
            log::info!("[Collections] 书架 {} 条", list.len());
            batch(|| {
                entries.set(list.clone());
                visuals.set(HashMap::new());
                phase.set(Phase::Loaded);
            });
            for entry in list {
                let sid = entry.series_id;
                if sid.is_empty() {
                    continue;
                }
                let (visuals, state) = (visuals, crate::state::state());
                spawn(async move {
                    let env = state.api_env();
                    let resolved = background(async move {
                        hongguo_service::series_service::resolver::resolve_series(&sid, &env).await
                    })
                    .await;
                    if let Ok(Ok(series)) = resolved {
                        visuals.update(|map| {
                            map.insert(
                                series.series_id.clone(),
                                Visual {
                                    title: series.title,
                                    episode_count: series.episode_count,
                                    cover: series.cover,
                                },
                            );
                        });
                    }
                });
            }
        }
        Ok(Err(e)) => {
            log::warn!("[Collections] 书架拉取失败: {e}");
            if entries.get().is_empty() {
                phase.set(Phase::Failed(e.to_string()));
            }
        }
        Err(e) => {
            log::warn!("[Collections] 书架任务失败: {e}");
            if entries.get().is_empty() {
                phase.set(Phase::Failed(e.to_string()));
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
                .placeholder(move || t("collections.searchPlaceholder"))
                .placeholder_color(crate::tokens::icon_muted())
                .value(move || value.get())
                .max_length(50)
                .on_change_text(move |v: &str| value.set(v.to_owned())),
        )
}

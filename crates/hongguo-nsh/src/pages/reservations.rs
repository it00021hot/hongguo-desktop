//! 预约页（Tauri `/reservations` / GPUI `pages/reservations` 的 NSH 对齐）。
//!
//! 「已上线 / 待上线」双 tab（服务端预约列表扁平形态，tab_type=13 的
//! online_total/offline_total 计数）：行卡 = 封面（待上线带「MM-dd 上线」
//! 角标文案行）+ 标题；已上线整行点击续播，待上线行内「取消预约」
//! （`reserve_series(sid,false)`）。两 tab 首页并行拉（GPUI issue #11 同款：
//! 不串行拉全，首屏快）；搜索对当前 tab 的标题过滤。批量删除预约留 P3。

use chrono::Local;
use nsh_core::ui::prelude::*;

use hongguo_api::calendar::CalendarItem;

use crate::i18n::t;
use crate::nav::{Nav, Page};
use crate::pages::common::{
    PAGE_PAD, Phase, empty_body, failed_body, loading_body, login_body, pill,
};

/// 行卡封面（3:4 小图）。
const COVER_W: f32 = 96.0;
const COVER_H: f32 = 128.0;

/// 页面双 tab（true = 已上线 / false = 待上线）。
const TABS: [(&str, &str, bool); 2] = [
    ("reservation.tab.online", "res-tab-online", true),
    ("reservation.tab.offline", "res-tab-offline", false),
];

#[component]
pub fn ReservationsPage() -> impl IntoElement {
    let online_items: Signal<Vec<CalendarItem>> = signal_with(Vec::new(), SignalEquals::Never);
    let offline_items: Signal<Vec<CalendarItem>> = signal_with(Vec::new(), SignalEquals::Never);
    let online_total = signal(0i64);
    let offline_total = signal(0i64);
    let phase = signal(Phase::Loading);
    let tab = signal(true);
    let search = signal(String::new());
    let loading = stored(false);
    let nav = Nav::from_context();
    let logged = crate::account::logged_in();

    // 挂载拉一次；登录态翻沿重拉（未登录给登录墙）。
    {
        effect(move || {
            logged.get();
            let (online_items, offline_items, online_total, offline_total, phase, loading) = (
                online_items,
                offline_items,
                online_total,
                offline_total,
                phase,
                loading,
            );
            spawn(async move {
                load(
                    online_items,
                    offline_items,
                    online_total,
                    offline_total,
                    phase,
                    loading,
                )
                .await;
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
        // 工具行：双 tab（带计数）+ 搜索框。
        .child(
            view()
                .w_full()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .children(TABS.map(|(key, id, online)| {
                    let active = move || tab.get() == online;
                    let count = move || {
                        if online {
                            online_total.get()
                        } else {
                            offline_total.get()
                        }
                    };
                    pill(
                        id,
                        move || format!("{} {}", t(key), count()),
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
                        move || tab.set(online),
                    )
                }))
                .child(view().flex_1())
                .child(search_box(search)),
        )
        // 主体：阶段态 / 列表。
        .child(If::new(
            move || phase.get() == Phase::Loading,
            loading_body,
        ))
        .child(If::new(
            move || phase.get() == Phase::LoginRequired,
            move || login_body("reservation.loginRequired"),
        ))
        .child(If::new(
            move || matches!(phase.get(), Phase::Failed(_)),
            move || {
                let Phase::Failed(message) = phase.get() else {
                    return view().into_element();
                };
                let message = signal(message);
                failed_body(message, move || {
                    let (online_items, offline_items, online_total, offline_total, phase, loading) = (
                        online_items,
                        offline_items,
                        online_total,
                        offline_total,
                        phase,
                        loading,
                    );
                    spawn(async move {
                        load(
                            online_items,
                            offline_items,
                            online_total,
                            offline_total,
                            phase,
                            loading,
                        )
                        .await;
                    });
                })
                .into_element()
            },
        ))
        .child(If::new(
            move || phase.get() == Phase::Loaded,
            move || {
                let items = current_items(online_items, offline_items, tab);
                if items.is_empty() {
                    return empty_body("reservation.empty", Some("reservation.emptyHint"))
                        .into_element();
                }
                let rows = filter_rows(&items, search);
                if rows.is_empty() {
                    return empty_body("reservation.empty", None).into_element();
                }
                scroll()
                    .flex_1()
                    .w_full()
                    .flex_col()
                    .gap(px(10.0))
                    .child(Each::new(
                        move || {
                            filter_rows(
                                &current_items(online_items, offline_items, tab),
                                search,
                            )
                        },
                        |item: &CalendarItem| item.series_id.clone(),
                        move |item: ReadSignal<CalendarItem>, _| {
                            row(
                                nav,
                                item,
                                tab,
                                online_items,
                                offline_items,
                                online_total,
                                offline_total,
                                phase,
                                loading,
                            )
                        },
                    ))
                    .into_element()
            },
        ))
}

/// 当前 tab 的条目（已上线 / 待上线）。
fn current_items(
    online_items: Signal<Vec<CalendarItem>>,
    offline_items: Signal<Vec<CalendarItem>>,
    tab: Signal<bool>,
) -> Vec<CalendarItem> {
    if tab.get() {
        online_items.get()
    } else {
        offline_items.get()
    }
}

/// 标题/seriesId 过滤（Tauri `matchListQuery` 同口径）。
fn filter_rows(items: &[CalendarItem], search: Signal<String>) -> Vec<CalendarItem> {
    let q = search.get().trim().to_lowercase();
    items
        .iter()
        .filter(|item| {
            q.is_empty()
                || item.title.to_lowercase().contains(&q)
                || item.series_id.to_lowercase().contains(&q)
        })
        .cloned()
        .collect()
}

/// 一行预约：封面（待上线带「MM-dd 上线」行）+ 标题 + 取消预约。
#[expect(
    clippy::too_many_arguments,
    reason = "重拉句柄随行卡下发，NSH 页面无实体可挂"
)]
fn row(
    nav: Nav,
    item: ReadSignal<CalendarItem>,
    tab: Signal<bool>,
    online_items: Signal<Vec<CalendarItem>>,
    offline_items: Signal<Vec<CalendarItem>>,
    online_total: Signal<i64>,
    offline_total: Signal<i64>,
    phase: Signal<Phase>,
    loading: Stored<bool>,
) -> impl IntoElement {
    let sid = item.get().series_id.clone();
    let key = format!("res-{}", sid);
    let play_sid = sid.clone();
    let cancel_sid = sid.clone();
    let online = item.get().is_online;
    view()
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .p(px(8.0))
        .rounded(px(10.0))
        .bg(theme().group_box)
        .child(
            // 封面 3:4 小图（封面缺省给纯色底）。
            view()
                .w(px(COVER_W))
                .h(px(COVER_H))
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
                // 排期行：已上线给「看剧」提示，待上线给「MM-dd 上线」。
                .child(
                    text(move || badge_line(&item.get()))
                        .text_size(px(12.0))
                        .text_color(theme().muted_foreground),
                ),
        )
        // 待上线：行内取消预约。
        .child(If::new(
            move || tab.get() && !online,
            move || {
                let cancel_sid = cancel_sid.clone();
                view()
                    .h(px(28.0))
                    .flex_row()
                    .items_center()
                    .px(px(12.0))
                    .rounded(px(999.0))
                    .border_1()
                    .border_color(move || theme().danger)
                    .key("res-cancel")
                    .on_press(move |_: &()| {
                        cancel_reserve(
                            &cancel_sid,
                            online_items,
                            offline_items,
                            online_total,
                            offline_total,
                            phase,
                            loading,
                        )
                    })
                    .child(
                        text(move || t("reservation.cancel"))
                            .text_size(px(12.0))
                            .text_color(move || theme().danger),
                    )
            },
        ))
        // 已上线整行点击续播（没看过从第 1 集起）。
        .key(key)
        .on_press(move |_: &()| {
            if !item.get().is_online {
                return;
            }
            let state = crate::state::state();
            let (vid_index, start_secs) =
                hongguo_service::play_service::resume::resolve_entry(&state.store, &play_sid, &[])
                    .ok()
                    .unwrap_or((1, 0.0));
            crate::play_req::request(play_sid.clone(), vid_index, start_secs);
            nav.goto(Page::Home);
        })
}

/// 排期行文案：已上线 = `reservation.publishAt`？否——已上线直接「看剧」
/// 提示；待上线 = 「MM-dd 上线」（未定档给占位说明）。
fn badge_line(item: &CalendarItem) -> String {
    if item.is_online {
        t("reservation.watch")
    } else if item.publish_time > 0 {
        t("reservation.upcomingBadge").replace("{date}", &format_month_day(item.publish_time))
    } else {
        t("reservation.publishAt").replace("{time}", "--")
    }
}

/// UTC 毫秒 → 本地 "MM-dd"（GPUI `format_month_day` 同口径）。
fn format_month_day(ms: i64) -> String {
    use chrono::TimeZone;
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|date| date.format("%m-%d").to_string())
        .unwrap_or_else(|| "--".to_owned())
}

/// 单条取消预约，成功后重拉两 tab（计数随服务端回包刷新）。
fn cancel_reserve(
    series_id: &str,
    online_items: Signal<Vec<CalendarItem>>,
    offline_items: Signal<Vec<CalendarItem>>,
    online_total: Signal<i64>,
    offline_total: Signal<i64>,
    phase: Signal<Phase>,
    loading: Stored<bool>,
) {
    let state = crate::state::state();
    let sid = series_id.to_owned();
    spawn(async move {
        let env = state.api_env();
        let result =
            background(
                async move { hongguo_api::reservation::reserve_series(&sid, false, &env).await },
            )
            .await;
        match result {
            Ok(Ok(())) => {
                toast(t("reservation.cancelled"), ToastDuration::Short);
                load(
                    online_items,
                    offline_items,
                    online_total,
                    offline_total,
                    phase,
                    loading,
                )
                .await;
            }
            Ok(Err(e)) => log::warn!("[Reservations] 取消预约失败: {e}"),
            Err(e) => log::warn!("[Reservations] 取消预约任务失败: {e}"),
        }
    });
}

/// 双 tab 首页并行拉（GPUI issue #11 同款：不串行拉全）。
async fn load(
    online_items: Signal<Vec<CalendarItem>>,
    offline_items: Signal<Vec<CalendarItem>>,
    online_total: Signal<i64>,
    offline_total: Signal<i64>,
    phase: Signal<Phase>,
    loading: Stored<bool>,
) {
    if loading.with(|v| *v) {
        return;
    }
    loading.replace(true);
    let state = crate::state::state();
    if state.settings().account.is_none() {
        batch(|| {
            online_items.set(Vec::new());
            offline_items.set(Vec::new());
            phase.set(Phase::LoginRequired);
        });
        loading.replace(false);
        return;
    }
    phase.set(Phase::Loading);
    let env = state.api_env();
    let fetch = |online: bool| {
        let env = env.clone();
        async move { hongguo_api::reservation::fetch_reservations_page(online, 0, None, &env).await }
    };
    let (online_result, offline_result) = (background(fetch(true)), background(fetch(false)));
    let (online_result, offline_result) = (online_result.await, offline_result.await);
    loading.replace(false);
    let mut failure: Option<String> = None;
    match online_result {
        Ok(Ok(page)) => {
            online_total.set(page.online_total);
            online_items.set(page.items);
        }
        Ok(Err(e)) => failure = Some(e.to_string()),
        Err(e) => failure = Some(e.to_string()),
    }
    match offline_result {
        Ok(Ok(page)) => {
            offline_total.set(page.offline_total);
            offline_items.set(page.items);
        }
        Ok(Err(e)) if failure.is_none() => failure = Some(e.to_string()),
        Ok(Err(_)) => {}
        Err(e) if failure.is_none() => failure = Some(e.to_string()),
        Err(_) => {}
    }
    match failure {
        None => {
            log::info!(
                "[Reservations] 已上线 {} 条 / 待上线 {} 条",
                online_items.get().len(),
                offline_items.get().len()
            );
            phase.set(Phase::Loaded);
        }
        Some(e) => {
            log::warn!("[Reservations] 预约列表失败: {e}");
            phase.set(Phase::Failed(e));
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
                .placeholder(move || t("reservation.searchPlaceholder"))
                .placeholder_color(crate::tokens::icon_muted())
                .value(move || value.get())
                .max_length(50)
                .on_change_text(move |v: &str| value.set(v.to_owned())),
        )
}

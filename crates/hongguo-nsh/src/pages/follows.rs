//! 追更页（Tauri `follows-page` / GPUI `pages/follows` 的 NSH 对齐）。
//!
//! 「追更的剧」列表（`follow_service::list`，服务端跟随列表）：行卡 = 封面、
//! 标题、状态行（已追平 / N 集未看 · 连载中/已完结）、集数进度，行内
//! 「取消追更」（`follow_service::toggle` off，连带删本地基线）。
//!
//! 追更必须登录（服务端列表），未登录给登录墙。更新提醒 tab（本地通知列
//! 表 / 检查更新）随 P3 迁入——顶栏状态字已带未读条数兜底。

use nsh_core::ui::prelude::*;

use hongguo_api::interact::FollowUpdateItem;

use crate::i18n::t;
use crate::nav::{Nav, Page};
use crate::pages::common::{PAGE_PAD, Phase, empty_body, failed_body, loading_body, login_body};

/// 行卡封面（3:4 小图）。
const COVER_W: f32 = 96.0;
const COVER_H: f32 = 128.0;

#[component]
pub fn FollowsPage() -> impl IntoElement {
    let items: Signal<Vec<FollowUpdateItem>> = signal_with(Vec::new(), SignalEquals::Never);
    let phase = signal(Phase::Loading);
    let loading = stored(false);
    let nav = Nav::from_context();
    let logged = crate::account::logged_in();

    // 挂载拉一次；登录态翻沿重拉（未登录给登录墙）。
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
        // 主体：阶段态 / 列表。
        .child(If::new(move || phase.get() == Phase::Loading, loading_body))
        .child(If::new(
            move || phase.get() == Phase::LoginRequired,
            move || login_body("follow.loginRequired"),
        ))
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
                    return empty_body("follow.empty", Some("follow.emptyHint")).into_element();
                }
                scroll()
                    .flex_1()
                    .w_full()
                    .flex_col()
                    .gap(px(10.0))
                    .child(Each::new(
                        move || items.get(),
                        |item: &FollowUpdateItem| item.series_id.clone(),
                        move |item: ReadSignal<FollowUpdateItem>, _| row(nav, item, items),
                    ))
                    .into_element()
            },
        ))
}

/// 一行追更：封面 + 标题 + 状态行 + 集数进度 + 取消追更。
fn row(
    nav: Nav,
    item: ReadSignal<FollowUpdateItem>,
    all: Signal<Vec<FollowUpdateItem>>,
) -> impl IntoElement {
    let sid = item.get().series_id.clone();
    let key = format!("follow-{}", sid);
    let play_sid = sid.clone();
    let unfollow_sid = sid.clone();
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
                .child(If::new(
                    move || !status_line(&item.get()).is_empty(),
                    move || {
                        text(move || status_line(&item.get()))
                            .text_size(px(12.0))
                            .text_color(theme().muted_foreground)
                    },
                ))
                .child(If::new(
                    move || item.get().episode_cnt > 0,
                    move || {
                        text(move || {
                            t("follow.episodeProgress")
                                .replace(
                                    "{watched}",
                                    &item.get().latest_watched_vid_index.to_string(),
                                )
                                .replace("{total}", &item.get().episode_cnt.to_string())
                        })
                        .text_size(px(12.0))
                        .text_color(theme().muted_foreground)
                    },
                )),
        )
        .child(
            text(move || t("follow.unfollow"))
                .text_size(px(12.0))
                .text_color(move || theme().danger)
                .px(px(12.0))
                .py(px(5.0))
                .rounded(px(999.0))
                .border_1()
                .border_color(move || theme().danger)
                .key("follow-remove")
                .on_press(move |_: &()| unfollow(&unfollow_sid, all)),
        )
        // 整行点击续播（服务端已看到第几集；0 = 没看过从第 1 集起）。
        .key(key)
        .on_press(move |_: &()| {
            let watched = item.get().latest_watched_vid_index.max(1);
            crate::play_req::request(play_sid.clone(), watched as u32, 0.0);
            nav.goto(Page::Home);
        })
}

/// 状态行（player 拼装同款）：已追平 / N 集未看 · 连载中/已完结。
fn status_line(item: &FollowUpdateItem) -> String {
    let remain = item.episode_cnt - item.latest_watched_vid_index;
    let progress = if item.latest_watched_vid_index > 0 {
        if remain <= 0 {
            t("follow.caughtUp")
        } else {
            t("follow.unwatched").replace("{count}", &remain.to_string())
        }
    } else {
        String::new()
    };
    let serial = if item.finished {
        t("follow.finished")
    } else {
        t("follow.serializing")
    };
    [progress, serial]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// 取消追更（连带删本地基线），成功后重拉。
fn unfollow(series_id: &str, all: Signal<Vec<FollowUpdateItem>>) {
    let state = crate::state::state();
    let sid = series_id.to_owned();
    spawn(async move {
        let arg = sid.clone();
        let result = background(async move {
            hongguo_service::follow_service::toggle(&state, &arg, false).await
        })
        .await;
        match result {
            Ok(Ok(())) => {
                toast(t("follow.unfollowed"), ToastDuration::Short);
                all.update(|list| list.retain(|it| it.series_id != sid));
            }
            Ok(Err(e)) => log::warn!("[Follows] 取消追更失败: {e}"),
            Err(e) => log::warn!("[Follows] 取消追更任务失败: {e}"),
        }
    });
}

/// 拉追更列表首页（登录墙在页面侧先挡：未登录不发请求）。
async fn load(items: Signal<Vec<FollowUpdateItem>>, phase: Signal<Phase>, loading: Stored<bool>) {
    if loading.with(|v| *v) {
        return;
    }
    loading.replace(true);
    let state = crate::state::state();
    if state.settings().account.is_none() {
        batch(|| {
            items.set(Vec::new());
            phase.set(Phase::LoginRequired);
        });
        loading.replace(false);
        return;
    }
    if items.get().is_empty() {
        phase.set(Phase::Loading);
    }
    let result =
        background(async move { hongguo_service::follow_service::list(&state, "").await }).await;
    loading.replace(false);
    match result {
        Ok(Ok(page)) => {
            log::info!("[Follows] 追更 {} 条", page.items.len());
            items.set(page.items);
            phase.set(Phase::Loaded);
        }
        Ok(Err(e)) => {
            log::warn!("[Follows] 追更列表失败: {e}");
            if items.get().is_empty() {
                phase.set(Phase::Failed(e.to_string()));
            }
        }
        Err(e) => {
            log::warn!("[Follows] 追更任务失败: {e}");
            if items.get().is_empty() {
                phase.set(Phase::Failed(e.to_string()));
            }
        }
    }
}

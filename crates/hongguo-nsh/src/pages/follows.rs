//! 追更页（Tauri `follows-page` / GPUI `pages/follows` 的 NSH 对齐）。
//!
//! 双 tab（Tauri 会话级保留 tab）：
//! - **追更的剧**：服务端跟随列表（`follow_service::list`），行卡 = 封面、
//!   标题、状态行（已追平 / N 集未看 · 连载中/已完结）、集数进度，行内
//!   「取消追更」（`follow_service::toggle` off，连带删本地基线）；
//! - **更新提醒**：本地通知（`follow_service::notices`），点条目跳播放并
//!   标已读；「检查更新」手动触发一轮（20 分钟定时器已在 service 跑）；
//!   「清空」清列表。
//!
//! 追更必须登录（服务端列表），未登录给登录墙。打开通知 tab 时顶部状态
//! 字的「新增提醒」计数归零（`pump::reset_follow_notices`）。

use nsh_core::ui::prelude::*;

use hongguo_api::interact::FollowUpdateItem;
use hongguo_common::model::FollowNotice;

use crate::i18n::t;
use crate::nav::{Nav, Page};
use crate::pages::common::{
    PAGE_PAD, Phase, empty_body, failed_body, loading_body, login_body, pill,
};
use crate::player::widgets::relative_time;

/// 行卡封面（3:4 小图）。
const COVER_W: f32 = 96.0;
const COVER_H: f32 = 128.0;

/// 页面双 tab（0 追更的剧 / 1 更新提醒）。
const TABS: [(&str, &str, u8); 2] = [
    ("follow.tabFollows", "follow-tab-0", 0),
    ("follow.tabNotices", "follow-tab-1", 1),
];

#[component]
pub fn FollowsPage() -> impl IntoElement {
    let items: Signal<Vec<FollowUpdateItem>> = signal_with(Vec::new(), SignalEquals::Never);
    let notices: Signal<Vec<FollowNotice>> = signal_with(Vec::new(), SignalEquals::Never);
    let phase = signal(Phase::Loading);
    let tab = signal(0u8);
    let checking = signal(false);
    let loading = stored(false);
    let nav = Nav::from_context();
    let logged = crate::account::logged_in();
    let events = crate::pump::events();

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
    // 通知列表：挂载 + 任一业务事件到达重拉（本地读，直读即刷；事件粒度
    // 对齐 pump 的 SignalEquals::Never——通知批、登录态都会翻这个信号）。
    {
        effect(move || {
            events.get();
            logged.get();
            let notices = notices;
            refresh_notices(notices);
        });
    }
    // 打开通知 tab：顶部状态字的新增提醒计数归零。
    {
        effect(move || {
            if tab.get() == 1 {
                crate::pump::reset_follow_notices();
            }
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
        // 工具行：双 tab + （通知 tab 的）检查更新/清空。
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
                .child(view().flex_1())
                .child(If::new(
                    move || tab.get() == 1 && phase.get() == Phase::Loaded,
                    move || {
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .child(pill(
                                "follow-check",
                                move || {
                                    if checking.get() {
                                        t("follow.checking")
                                    } else {
                                        t("follow.checkUpdate")
                                    }
                                },
                                move || theme().muted,
                                move || theme().foreground,
                                move || check_updates(checking, notices),
                            ))
                            .child(pill(
                                "follow-clear",
                                move || t("follow.clearAll"),
                                move || theme().muted,
                                move || theme().muted_foreground,
                                move || clear_notices(notices),
                            ))
                    },
                )),
        )
        // 追更 tab 主体：阶段态 / 列表。
        .child(If::new(
            move || tab.get() == 0 && phase.get() == Phase::Loading,
            loading_body,
        ))
        .child(If::new(
            move || tab.get() == 0 && phase.get() == Phase::LoginRequired,
            move || login_body("follow.loginRequired"),
        ))
        .child(If::new(
            move || tab.get() == 0 && matches!(phase.get(), Phase::Failed(_)),
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
            move || tab.get() == 0 && phase.get() == Phase::Loaded,
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
        // 通知 tab 主体：空态 / 列表。
        .child(If::new(
            move || tab.get() == 1,
            move || {
                if notices.get().is_empty() {
                    return empty_body("follow.noticesEmpty", Some("follow.noticesEmptyHint"))
                        .into_element();
                }
                scroll()
                    .flex_1()
                    .w_full()
                    .flex_col()
                    .gap(px(8.0))
                    .child(Each::new(
                        move || notices.get(),
                        |notice: &FollowNotice| notice.id,
                        move |notice: ReadSignal<FollowNotice>, _| notice_row(nav, notice, notices),
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
            view()
                .h(px(28.0))
                .flex_row()
                .items_center()
                .px(px(12.0))
                .rounded(px(999.0))
                .border_1()
                .border_color(move || theme().danger)
                .key("follow-remove")
                .on_press(move |_: &()| unfollow(&unfollow_sid, all))
                .child(
                    text(move || t("follow.unfollow"))
                        .text_size(px(12.0))
                        .text_color(move || theme().danger),
                ),
        )
        // 整行点击续播（服务端已看到第几集；0 = 没看过从第 1 集起）。
        .key(key)
        .on_press(move |_: &()| {
            let watched = item.get().latest_watched_vid_index.max(1);
            crate::play_req::request(play_sid.clone(), watched as u32, 0.0);
            nav.goto(Page::Home);
        })
}

/// 一条通知：未读点 + 标题 + 正文 + 相对时间；整行点击跳播放并标已读。
fn notice_row(
    nav: Nav,
    notice: ReadSignal<FollowNotice>,
    all: Signal<Vec<FollowNotice>>,
) -> impl IntoElement {
    let id = notice.get().id;
    let sid = notice.get().series_id.clone();
    let key = format!("notice-{id}");
    view()
        .w_full()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .p(px(10.0))
        .rounded(px(10.0))
        .bg(theme().group_box)
        // 未读点（已读位灰点占位，行高不跳）。
        .child(view().w(px(8.0)).h(px(8.0)).rounded(px(999.0)).bg(move || {
            if notice.get().read {
                theme().muted
            } else {
                theme().warning
            }
        }))
        .child(
            view()
                .flex_1()
                .overflow_hidden()
                .flex_col()
                .gap(px(3.0))
                .child(
                    text(move || {
                        let title = notice.get().title;
                        if title.is_empty() {
                            t("history.unknownTitle")
                        } else {
                            title
                        }
                    })
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme().foreground),
                )
                .child(
                    text(move || notice.get().detail)
                        .text_size(px(12.0))
                        .text_color(theme().muted_foreground),
                ),
        )
        .child(
            text(move || relative_time(notice.get().at * 1000))
                .text_size(px(12.0))
                .text_color(theme().muted_foreground),
        )
        .key(key)
        .on_press(move |_: &()| {
            mark_read(id, all);
            if !sid.is_empty() {
                crate::play_req::request(sid.clone(), 1, 0.0);
                nav.goto(Page::Home);
            }
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

/// 标一条已读（本地同步改，避免整列表重拉闪烁）。
fn mark_read(id: i64, all: Signal<Vec<FollowNotice>>) {
    let state = crate::state::state();
    if let Err(e) = hongguo_service::follow_service::mark_notices_read(&state, &[id]) {
        log::warn!("[Follows] 标已读失败: {e}");
    }
    all.update(|list| {
        for notice in list.iter_mut() {
            if notice.id == id {
                notice.read = true;
            }
        }
    });
}

/// 手动检查一轮更新（20 分钟定时器之外的立即回音入口）。
fn check_updates(checking: Signal<bool>, notices: Signal<Vec<FollowNotice>>) {
    if checking.get() {
        return;
    }
    checking.set(true);
    let state = crate::state::state();
    spawn(async move {
        let result =
            background(async move { hongguo_service::follow_service::check_updates(&state).await })
                .await;
        checking.set(false);
        match result {
            Ok(Ok(0)) => toast(t("follow.checkNone"), ToastDuration::Short),
            Ok(Ok(count)) => {
                toast(
                    t("follow.checkResult").replace("{count}", &count.to_string()),
                    ToastDuration::Short,
                );
                refresh_notices(notices);
            }
            Ok(Err(e)) => log::warn!("[Follows] 检查更新失败: {e}"),
            Err(e) => log::warn!("[Follows] 检查更新任务失败: {e}"),
        }
    });
}

/// 清空通知（本地操作），成功后重拉。
fn clear_notices(notices: Signal<Vec<FollowNotice>>) {
    let state = crate::state::state();
    if let Err(e) = hongguo_service::follow_service::clear_notices(&state) {
        log::warn!("[Follows] 清空通知失败: {e}");
        return;
    }
    toast(t("follow.cleared"), ToastDuration::Short);
    refresh_notices(notices);
}

/// 重拉本地通知（同步读；调用处不在渲染闭包内即可）。
fn refresh_notices(notices: Signal<Vec<FollowNotice>>) {
    let state = crate::state::state();
    match hongguo_service::follow_service::notices(&state) {
        Ok(list) => notices.set(list),
        Err(e) => log::warn!("[Follows] 通知读取失败: {e}"),
    }
}

/// 取消追更（连带删本地基线），成功后摘行。
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

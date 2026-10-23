//! 播放页互动栏：收藏 / 评论 / 点赞 / 追更 / 分享（抖音系右缘竖排形态）。
//!
//! icon 在上、计数在下，实心纯白贴画面右缘（`interaction-rail.tsx` 同款）。
//! 计数来自分集档案的 detail 公开计数（匿名可见），登录后互动回显兜底
//! 「已赞/已收藏」红标；写操作走 `hongguo_api::interact` 的同一批调用
//! （点赞 do_action 3/4、收藏 bookshelf 0/1、追更 do_action 34/35），
//! 乐观翻面、失败回退。分享 = 剪贴板复制链接。

use nsh_core::ui::prelude::*;

use hongguo_service::app_state::AppState;

use crate::i18n::t;

use super::{icons, widgets};

/// 互动栏的全部动态状态（计数由 `mod.rs` 在档案到达时写入）。
#[derive(Clone, Copy)]
pub(crate) struct RailState {
    pub digged: Signal<bool>,
    pub collected: Signal<bool>,
    pub following: Signal<bool>,
    /// 追更钮显隐：官方只在连载剧（series_status=0）出追更；meta 未到
    /// （false 起步）先不显示，档案到了再翻面。
    pub show_follow: Signal<bool>,
    pub comment_count: Signal<i64>,
    pub digg_count: Signal<i64>,
    pub collect_count: Signal<i64>,
}

/// 互动栏的上下文（身份与对象）。
#[derive(Clone, Copy)]
pub(crate) struct RailCtx {
    pub series_id: Signal<String>,
    /// 当前集裸 vid（点赞对象；空 = 档案未就绪，点赞禁用）。
    pub bare_vid: Signal<String>,
    pub title: Signal<String>,
    pub logged_in: Signal<bool>,
    /// 评论入口：点亮评论面板。
    pub open_comments: Signal<bool>,
}

impl RailState {
    pub(crate) fn new() -> Self {
        Self {
            digged: signal(false),
            collected: signal(false),
            following: signal(false),
            show_follow: signal(false),
            comment_count: signal(0),
            digg_count: signal(0),
            collect_count: signal(0),
        }
    }
}

/// 画面右缘竖排互动栏。顺序对齐抖音：收藏 → 评论 → 点赞 → 追更 → 分享。
pub(crate) fn rail(s: RailState, ctx: RailCtx) -> impl IntoElement {
    view()
        .flex_col()
        .items_center()
        .gap(px(16.0))
        .child(rail_item(
            "collect",
            move || {
                let on = s.collected.get();
                let c = if on { rgb(0xfbbf24) } else { rgb(0xffffff) };
                icons::star_fill(26.0, move || c)
            },
            move || {
                let count = s.collect_count.get();
                (count > 0).then(|| widgets::fmt_count(count))
            },
            move || on_collect(s, ctx),
        ))
        .child(rail_item(
            "comments",
            move || {
                let open = ctx.open_comments.get();
                let c = if open { rgb(0xef4444) } else { rgb(0xffffff) };
                icons::comment_fill(30.0, move || c)
            },
            move || {
                let count = s.comment_count.get();
                (count > 0).then(|| widgets::fmt_count(count))
            },
            move || ctx.open_comments.update(|v| *v = true),
        ))
        .child(rail_item(
            "like",
            move || {
                let on = s.digged.get();
                let c = if on { rgb(0xef4444) } else { rgb(0xffffff) };
                icons::heart_fill(26.0, move || c)
            },
            move || {
                let count = s.digg_count.get();
                (count > 0).then(|| widgets::fmt_count(count))
            },
            move || on_digg(s, ctx),
        ))
        // 追更钮只在连载剧显示（完结剧不会有新集）。
        .child(If::new(
            move || s.show_follow.get(),
            move || {
                rail_item(
                    "follow",
                    move || {
                        let on = s.following.get();
                        let c = if on { rgb(0xf97316) } else { rgb(0xffffff) };
                        icons::tv(26.0, move || c)
                    },
                    move || {
                        Some(if s.following.get() {
                            t("player.interact.following")
                        } else {
                            t("player.interact.follow")
                        })
                    },
                    move || on_follow(s, ctx),
                )
            },
        ))
        .child(rail_item(
            "share",
            move || icons::share_fill(26.0, || rgb(0xffffff)),
            move || Some(t("player.interact.share")),
            move || on_share(ctx),
        ))
}

/// 一个互动栏单元：icon 在上、计数/文案在下。
fn rail_item<E: IntoElement + 'static>(
    key: &'static str,
    icon: impl Fn() -> E + 'static,
    label: impl Fn() -> Option<String> + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    view()
        .flex_col()
        .items_center()
        .gap(px(2.0))
        .child(icon())
        .child(
            text(move || label().unwrap_or_default())
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(0xffffff))
                .tabular_nums(),
        )
        .key(format!("rail-{key}"))
        .on_press(move |_: &()| on_press())
}

/// 未登录：指路提示（登录入口在设置页——NSH 壳 P2 迁设置页）。
fn require_login() {
    toast(t("player.interact.loginRequired"), ToastDuration::Long);
}

fn on_digg(s: RailState, ctx: RailCtx) {
    if !ctx.logged_in.get() {
        return require_login();
    }
    let vid = ctx.bare_vid.get();
    if vid.is_empty() {
        return;
    }
    let next = !s.digged.get();
    s.digged.set(next);
    s.digg_count
        .update(|v| *v = (*v + if next { 1 } else { -1 }).max(0));
    let state = crate::state::state();
    let series_id = ctx.series_id.get();
    spawn(async move {
        let result = background(async move {
            hongguo_api::interact::digg_video(&vid, &series_id, next, &state.api_env()).await
        })
        .await;
        match result {
            Ok(Ok(())) => {
                toast(
                    t(if next {
                        "player.interact.liked"
                    } else {
                        "player.interact.undone"
                    }),
                    ToastDuration::Short,
                );
            }
            Ok(Err(e)) => {
                s.digged.set(!next);
                s.digg_count
                    .update(|v| *v = (*v + if next { -1 } else { 1 }).max(0));
                log::warn!("[Rail] 点赞失败: {e}");
            }
            Err(e) => {
                s.digged.set(!next);
                s.digg_count
                    .update(|v| *v = (*v + if next { -1 } else { 1 }).max(0));
                log::warn!("[Rail] 点赞任务失败: {e}");
            }
        }
    });
}

fn on_collect(s: RailState, ctx: RailCtx) {
    if !ctx.logged_in.get() {
        return require_login();
    }
    let next = !s.collected.get();
    s.collected.set(next);
    s.collect_count
        .update(|v| *v = (*v + if next { 1 } else { -1 }).max(0));
    let state = crate::state::state();
    let series_id = ctx.series_id.get();
    spawn(async move {
        let result = background(async move {
            hongguo_api::interact::collect_series(&series_id, next, &state.api_env()).await
        })
        .await;
        match result {
            Ok(Ok(())) => {
                toast(
                    t(if next {
                        "player.interact.collected"
                    } else {
                        "player.interact.uncollected"
                    }),
                    ToastDuration::Short,
                );
            }
            Ok(Err(e)) => {
                s.collected.set(!next);
                s.collect_count
                    .update(|v| *v = (*v + if next { -1 } else { 1 }).max(0));
                log::warn!("[Rail] 收藏失败: {e}");
            }
            Err(e) => {
                s.collected.set(!next);
                s.collect_count
                    .update(|v| *v = (*v + if next { -1 } else { 1 }).max(0));
                log::warn!("[Rail] 收藏任务失败: {e}");
            }
        }
    });
}

fn on_follow(s: RailState, ctx: RailCtx) {
    if !ctx.logged_in.get() {
        return require_login();
    }
    let next = !s.following.get();
    s.following.set(next);
    let state = crate::state::state();
    let series_id = ctx.series_id.get();
    spawn(async move {
        let result = background(async move {
            hongguo_api::interact::follow_update(&series_id, next, &state.api_env()).await
        })
        .await;
        if let Ok(Err(e)) = result {
            s.following.set(!next);
            log::warn!("[Rail] 追更失败: {e}");
        }
    });
}

/// 分享：`剧名 链接` 进剪贴板（Tauri 同一条文案形态）。
fn on_share(ctx: RailCtx) {
    let title = ctx.title.get();
    let url = format!(
        "https://hongguoduanju.com/detail?series_id={}",
        ctx.series_id.get()
    );
    let text = format!("{} {url}", title.trim()).trim().to_owned();
    set_clipboard(text);
    toast(t("player.interact.shareCopied"), ToastDuration::Short);
}

/// 登录后拉一次互动回显，把「已赞/已收藏/已追更」红标翻到位
/// （计数仍以档案公开计数优先，这里只兜状态）。
pub(crate) async fn restore_state(s: RailState, ctx: RailCtx, state: AppState) {
    let (interact, follow) = (
        super::feed::interaction_state(&state).await,
        super::feed::follow_list(&state).await,
    );
    if let Ok(st) = interact {
        let series_id = ctx.series_id.get();
        let vid = ctx.bare_vid.get();
        let hit = st.items.iter().find(|i| i.vid == vid);
        let series_hit = st.items.iter().find(|i| i.series_id == series_id);
        batch(|| {
            if let Some(hit) = hit {
                s.digged.set(hit.user_digg);
                if s.digg_count.get() == 0 {
                    s.digg_count.set(hit.digged_count);
                }
            }
            if let Some(series_hit) = series_hit {
                s.collected.set(series_hit.followed);
                if s.collect_count.get() == 0 {
                    s.collect_count.set(series_hit.followed_cnt);
                }
            }
        });
    }
    if let Ok(page) = follow {
        let series_id = ctx.series_id.get();
        if let Some(item) = page.items.iter().find(|i| i.series_id == series_id) {
            s.following.set(true);
            // meta 未到时按追更列表判定连载状态（已追+完结=隐藏）。
            if !s.show_follow.get() && !item.finished {
                s.show_follow.set(true);
            }
        }
    }
}

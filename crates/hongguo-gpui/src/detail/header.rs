//! 详情页头部：返回栏 + 封面/标题/统计行/标签/简介/动作行 + 备案号。
//! 行构成对齐 Tauri `series-detail-page.tsx`（评分→热度→追剧→播放，
//! 评分取剧评首页 extra；缺失段不渲染不打断行）。

use std::sync::Arc;

use gpui_kit::component::WindowExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, SharedString, Window, div, img};
use hongguo_api::interact::{collect_series, digg_video};

use super::DetailView;
use crate::bridge;

/// 星 = 收藏/评分，红 = 热度/点赞——都走业务语义令牌（Tauri 详情页
/// `text-star` / `text-hot` 同款；亮暗分档在 `tokens` 里）。
pub(super) fn star_color(mode: gpui_kit::component::ThemeMode) -> gpui_kit::Rgba {
    crate::tokens::star(mode)
}
pub(super) fn hot_color(mode: gpui_kit::component::ThemeMode) -> gpui_kit::Rgba {
    crate::tokens::hot(mode)
}

impl DetailView {
    pub(super) fn render_back_bar(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        h_flex()
            .items_center()
            .gap_2()
            .px_4()
            .py_3()
            .child(
                Button::new("detail-back")
                    .icon(gpui_kit::assets::IconName::ChevronLeft)
                    .ghost()
                    .small()
                    .label(crate::i18n::t("detail.back"))
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(super::DetailEvent::Back);
                    })),
            )
            .into_any_element()
    }

    pub(super) fn render_header(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let series = self.series.as_ref().expect("render_header 仅就绪态调用");
        let meta = self.meta.as_ref();
        let cover = self.images.get(&series.cover).cloned();
        // 统计行（Tauri 排版：评分 评分人数 → 红果热度值 → 追剧 → 播放）
        let mut stats: Vec<String> = Vec::new();
        if !self.reviews_score.is_empty() {
            let mut seg = format!(
                "{:.1}{}",
                self.reviews_score.parse::<f64>().unwrap_or(0.0),
                crate::i18n::t("detail.scoreUnit")
            );
            if self.reviews_score_cnt > 0 {
                seg.push_str(&format!(
                    " {}",
                    crate::i18n::tf(
                        "detail.ratingCount",
                        &[("count", &Self::fmt_count(self.reviews_score_cnt))],
                    )
                ));
            }
            stats.push(seg);
        }
        if let Some(m) = meta {
            if m.hot_score > 0 {
                stats.push(format!(
                    "🔥{}",
                    crate::i18n::tf(
                        "detail.heatValue",
                        &[("count", &Self::fmt_count(m.hot_score))],
                    )
                ));
            }
            if m.followed_cnt > 0 {
                stats.push(crate::i18n::tf(
                    "detail.followCount",
                    &[("count", &Self::fmt_count(m.followed_cnt))],
                ));
            }
            if m.play_cnt > 0 {
                stats.push(crate::i18n::tf(
                    "detail.playsCount",
                    &[("count", &Self::fmt_count(m.play_cnt))],
                ));
            }
        }
        // 标签行：季徽（红底高亮）+ 全 N 集 + 题材标签（meta 的 secondary_infos
        // 优先，档案 tags 是解析兜底——Tauri 同一取序）
        let tags = meta
            .map(|m| m.tags.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| series.tags.clone());
        let (resume_ep, resume_start) = self.resume_entry();
        let intro = meta.map(|m| m.intro.clone()).unwrap_or_default();
        // 未上线降级视图：简介回落来源页 prefill 的描述（Tauri 同口径）
        let intro = if intro.is_empty() && self.upcoming {
            self.prefill
                .as_ref()
                .map(|p| p.desc.clone())
                .unwrap_or_default()
        } else {
            intro
        };

        h_flex()
            .items_start()
            .gap_8()
            .mt_4()
            .child(
                // 封面：w-44 (176px) 3:4 rounded-xl，Tauri SeriesCover 同位
                div()
                    .h(gpui_kit::px(234.0))
                    .w(gpui_kit::px(176.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded(gpui_kit::px(12.0))
                    .bg(theme.secondary)
                    .children(cover.map(|bitmap| {
                        img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                    })),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(gpui_kit::FontWeight::BOLD)
                            .child(SharedString::from(series.title.clone())),
                    )
                    .children((!stats.is_empty()).then(|| {
                        div()
                            .mt_2()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(stats.join("  ·  ")))
                    }))
                    .children((!tags.is_empty() || !series.episodes.is_empty()).then(|| {
                        h_flex()
                            .mt_3()
                            .flex_wrap()
                            .gap_1p5()
                            .children(
                                meta.filter(|m| !m.season.is_empty())
                                    .map(|m| Self::tag_pill(m.season.clone(), true, theme)),
                            )
                            .when(!series.episodes.is_empty(), |row| {
                                row.child(Self::tag_pill(
                                    crate::i18n::tf(
                                        "detail.allEpisodesBadge",
                                        &[("count", &series.episodes.len().to_string())],
                                    ),
                                    false,
                                    theme,
                                ))
                            })
                            .children(
                                tags.into_iter()
                                    .map(|t| Self::tag_pill(t, false, theme))
                                    .collect::<Vec<_>>(),
                            )
                    }))
                    .children((!intro.is_empty()).then(|| {
                        // 简介常驻全文（Tauri 无收起/展开，GPUI 旧版的展开
                        // 交互是分叉，删）
                        div()
                            .mt_4()
                            .text_sm()
                            .line_height(gpui_kit::relative(1.7))
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(intro))
                    }))
                    .child(self.render_creators(cx))
                    .child(if self.upcoming {
                        // 未上线降级视图（Tauri prefill 同款）：动作行换
                        // 「即将上线 + 预约（已预约可再点取消）」
                        h_flex()
                            .mt_5()
                            .items_center()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                div()
                                    .rounded_full()
                                    .bg(theme.secondary)
                                    .px_3()
                                    .py_1()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("⌛ {}", crate::i18n::t("player.comingSoon"))),
                            )
                            .child(
                                Button::new("detail-reserve")
                                    .icon(gpui_kit::assets::IconName::BellRing)
                                    .label(if self.reserved {
                                        crate::i18n::t("reservation.reserved")
                                    } else {
                                        crate::i18n::t("reservation.action")
                                    })
                                    .outline()
                                    .text_color(if self.reserved {
                                        theme.muted_foreground
                                    } else {
                                        theme.foreground
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_reserve(cx);
                                    })),
                            )
                            .into_any_element()
                    } else {
                        // 动作行：播放/继续播放 + 收藏 + 点赞 + 追更（仅连载
                        // 中显示）。收藏/点赞未登录给 toast 并进设置页账号卡；
                        // 追更沿用 Tauri follow 专用提示。
                        h_flex()
                            .mt_5()
                            .items_center()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                Button::new("detail-play")
                                    .icon(gpui_kit::assets::IconName::Play)
                                    .label(if resume_ep > 1 {
                                        // Tauri `detail.continueEpisode` / `playFirst`
                                        // 原文（含数字两侧空格），并排对拍逐字对齐
                                        crate::i18n::tf(
                                            "detail.continueEpisode",
                                            &[("index", &resume_ep.to_string())],
                                        )
                                    } else {
                                        crate::i18n::t("detail.playFirst")
                                    })
                                    .primary()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let start = if resume_start > 3.0 {
                                            resume_start
                                        } else {
                                            0.0
                                        };
                                        cx.emit(super::DetailEvent::Play {
                                            series_id: this.series_id.clone(),
                                            vid_index: resume_ep,
                                            start_secs: start,
                                        });
                                    })),
                            )
                            .child({
                                let collected = self.collected();
                                Button::new("detail-collect")
                                    .icon(gpui_kit::assets::IconName::Star)
                                    .label(if collected {
                                        crate::i18n::t("detail.collected")
                                    } else {
                                        crate::i18n::t("detail.collect")
                                    })
                                    .outline()
                                    .text_color(if collected {
                                        star_color(theme.mode).into()
                                    } else {
                                        theme.foreground
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_collect(window, cx);
                                    }))
                            })
                            .child({
                                let digged = self.digged();
                                Button::new("detail-digg")
                                    .icon(gpui_kit::assets::IconName::Heart)
                                    .label(crate::i18n::t("player.interact.like"))
                                    .outline()
                                    .text_color(if digged {
                                        hot_color(theme.mode).into()
                                    } else {
                                        theme.foreground
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_digg(window, cx);
                                    }))
                            })
                            .children((self.follow_visible()).then(|| {
                                let on = self.follow_is_on.unwrap_or(false);
                                Button::new("detail-follow")
                                    .icon(gpui_kit::assets::IconName::Tv)
                                    .label(if on {
                                        crate::i18n::t("follow.following")
                                    } else {
                                        crate::i18n::t("follow.follow")
                                    })
                                    .outline()
                                    .when(on, |b| {
                                        b.border_color(theme.primary).text_color(theme.primary)
                                    })
                                    .disabled(self.follow_pending)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_follow(window, cx);
                                    }))
                            }))
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }

    /// 出品方 / 演职员（Tauri `series-detail-page.tsx` creators 块同款：
    /// 头像 + 名字 + 角色行，点击跳创作者主页；无 uid 的旧条目仅展示）。
    pub(super) fn render_creators(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let Some(meta) = &self.meta else {
            return div().into_any_element();
        };
        if meta.creators.is_empty() {
            return div().into_any_element();
        }
        let mut row = h_flex().mt_4().flex_wrap().items_center().gap_3();
        for creator in &meta.creators {
            let clickable = !creator.user_id.is_empty();
            let avatar_view = match self.images.get(&creator.avatar).cloned() {
                Some(image) => img(ImageSource::Render(image))
                    .size_7()
                    .flex_shrink_0()
                    .rounded_full()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element(),
                None => h_flex()
                    .size_7()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(theme.muted)
                    .child(
                        gpui_kit::component::Icon::new(gpui_kit::assets::IconName::UserRound)
                            .small()
                            .text_color(theme.muted_foreground),
                    )
                    .into_any_element(),
            };
            let mut chip = div()
                .id(ElementId::from(format!(
                    "detail-creator-{}-{}",
                    creator.user_id, creator.name
                )))
                .flex()
                .items_center()
                .gap_2()
                .rounded_full()
                .border_1()
                .border_color(theme.border)
                .py_1()
                .pr_3()
                .pl_1()
                .tooltip({
                    let name = creator.name.clone();
                    move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(SharedString::from(
                            crate::i18n::tf("detail.creatorHome", &[("name", &name)]),
                        ))
                        .build(window, cx)
                    }
                });
            if clickable {
                let uid = creator.user_id.clone();
                chip = chip
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.secondary_hover))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(super::DetailEvent::OpenCreator(uid.clone()));
                    }));
            }
            row = row.child(
                chip.child(avatar_view).child(
                    v_flex()
                        .items_start()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui_kit::FontWeight::MEDIUM)
                                .child(SharedString::from(creator.name.clone())),
                        )
                        .children((!creator.role.is_empty()).then(|| {
                            div()
                                .text_size(gpui_kit::px(10.0))
                                .text_color(theme.muted_foreground)
                                .child(SharedString::from(creator.role.clone()))
                        })),
                ),
            );
        }
        row.into_any_element()
    }

    /// 备案号（头部块之下、tab 之上独立一行小字；空 = meta 未给不渲染）。
    pub(super) fn render_record_line(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let record = self
            .meta
            .as_ref()
            .map(|m| m.record_number.clone())
            .unwrap_or_default();
        div()
            .mt_4()
            .text_xs()
            .text_color(theme.muted_foreground)
            .when(record.is_empty(), |s| s.invisible())
            .child(SharedString::from(record))
            .into_any_element()
    }

    /// 标签 pill（季徽红底高亮 / 其余 secondary 底——Tauri Badge 同位）。
    fn tag_pill(
        text: String,
        hot: bool,
        theme: &gpui_kit::component::Theme,
    ) -> gpui_kit::AnyElement {
        div()
            .px_2()
            .py_0p5()
            .rounded_full()
            .text_xs()
            .when(hot, |s| {
                s.bg(hot_color(theme.mode))
                    .text_color(gpui_kit::rgb(0xffffff))
            })
            .when(!hot, |s| {
                s.bg(theme.secondary).text_color(theme.muted_foreground)
            })
            .child(SharedString::from(text))
            .into_any_element()
    }

    // —— 互动 ——

    /// 收藏态（mget 回显 best-effort，未命中按未收藏——Tauri
    /// useInteractionState 同口径）。
    fn collected(&self) -> bool {
        self.interact_items
            .iter()
            .find(|i| i.series_id == self.series_id)
            .map(|i| i.followed)
            .unwrap_or(false)
    }

    /// 当前集（继续播放指向的那集）的点赞态。
    fn digged(&self) -> bool {
        let (resume_ep, _) = self.resume_entry();
        let vid = self
            .series
            .as_ref()
            .and_then(|s| s.episodes.iter().find(|e| e.vid_index == resume_ep))
            .map(|e| e.vid.clone())
            .unwrap_or_default();
        if vid.is_empty() {
            return false;
        }
        self.interact_items
            .iter()
            .find(|i| i.vid == vid)
            .map(|i| i.user_digg)
            .unwrap_or(false)
    }

    fn toggle_collect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.collect_pending {
            return;
        }
        if !self.logged_in() {
            window.push_notification(
                Notification::info(crate::i18n::t("player.interact.loginRequired")),
                cx,
            );
            cx.emit(super::DetailEvent::OpenSettings);
            return;
        }
        let next = !self.collected();
        self.collect_pending = true;
        let sid = self.series_id.clone();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            collect_series(&sid, next, &env).await.map(|_| next)
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.collect_pending = false;
                match result {
                    Ok(_) => window.push_notification(
                        Notification::success(if next {
                            crate::i18n::t("player.interact.collected")
                        } else {
                            crate::i18n::t("player.interact.uncollected")
                        }),
                        cx,
                    ),
                    Err(e) => {
                        log::warn!("[Detail] 收藏失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        window.push_notification(
                            Notification::error(crate::i18n::tf(
                                "common.failedWith",
                                &[("message", message.as_str())],
                            )),
                            cx,
                        );
                    }
                }
                // 成败都刷新回显（成功取新态；失败回落服务端真相）
                view.load_interact_state(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn toggle_digg(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.digg_pending {
            return;
        }
        if !self.logged_in() {
            window.push_notification(
                Notification::info(crate::i18n::t("player.interact.loginRequired")),
                cx,
            );
            cx.emit(super::DetailEvent::OpenSettings);
            return;
        }
        let (resume_ep, _) = self.resume_entry();
        let Some(ep) = self
            .series
            .as_ref()
            .and_then(|s| s.episodes.iter().find(|e| e.vid_index == resume_ep))
        else {
            return;
        };
        if ep.vid.is_empty() {
            return;
        }
        let next = !self.digged();
        self.digg_pending = true;
        let sid = self.series_id.clone();
        let vid = ep.vid.clone();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            digg_video(&vid, &sid, next, &env).await.map(|_| next)
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.digg_pending = false;
                match result {
                    Ok(_) => window.push_notification(
                        Notification::success(if next {
                            crate::i18n::t("player.interact.liked")
                        } else {
                            crate::i18n::t("player.interact.undone")
                        }),
                        cx,
                    ),
                    Err(e) => {
                        log::warn!("[Detail] 点赞失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        window.push_notification(
                            Notification::error(crate::i18n::tf(
                                "common.failedWith",
                                &[("message", message.as_str())],
                            )),
                            cx,
                        );
                    }
                }
                view.load_interact_state(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 取证辅助：走真实未登录收藏路径（toast + 切设置页），供截图验回执。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_trigger_collect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_collect(window, cx);
    }

    /// 「追更」按钮显隐：官方只在连载剧（series_status=0）出追更钮——
    /// 完结剧「全 N 集」无钮（2026-10-15 实包口径，Tauri 同款）。
    pub(super) fn follow_visible(&self) -> bool {
        !self.upcoming && self.meta.as_ref().is_some_and(|m| m.series_status == 0)
    }

    /// 「是否已追」查询（跟随列表首页本地判成员，service 同一段；
    /// 未登录不查——点击时给登录墙回音）。
    pub(super) fn load_follow_state(&mut self, cx: &mut Context<Self>) {
        self.follow_is_on = None;
        if self.state.settings().account.is_none() {
            return;
        }
        let state = Arc::clone(&self.state);
        let sid = self.series_id.clone();
        let rx = bridge::spawn(async move {
            hongguo_service::follow_service::is_following(&state, &sid).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                if let Ok(on) = result {
                    view.follow_is_on = Some(on);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 追更切换（乐观翻转；失败回滚并透传错误原文；未登录给登录墙回音）。
    pub(super) fn toggle_follow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.follow_pending {
            return;
        }
        if self.state.settings().account.is_none() {
            window.push_notification(
                Notification::error(crate::i18n::t("follow.loginRequired")),
                cx,
            );
            return;
        }
        let next = !self.follow_is_on.unwrap_or(false);
        self.follow_is_on = Some(next);
        self.follow_pending = true;
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid = self.series_id.clone();
        let rx = bridge::spawn(async move {
            hongguo_service::follow_service::toggle(&state, &sid, next).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.follow_pending = false;
                match result {
                    Ok(()) => window.push_notification(
                        Notification::success(if next {
                            crate::i18n::t("follow.followed")
                        } else {
                            crate::i18n::t("follow.unfollowed")
                        }),
                        cx,
                    ),
                    Err(e) => {
                        // 失败回滚（下次重查拿服务端真相）
                        view.follow_is_on = Some(!next);
                        log::warn!("[Detail] 追更失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        window.push_notification(
                            Notification::error(crate::i18n::tf(
                                "common.failedWith",
                                &[("message", message.as_str())],
                            )),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 未上线降级视图的预约切换（Tauri prefill 视图「预约」按钮同款：
    /// 一个按钮管预约/取消预约，成败都弹通知）。
    fn toggle_reserve(&mut self, cx: &mut Context<Self>) {
        if self.reserve_pending {
            return;
        }
        self.reserve_pending = true;
        let next = !self.reserved;
        let state = Arc::clone(&self.state);
        let sid = self.series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::reservation::reserve_series(&sid, next, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| {
                view.reserve_pending = false;
                match result {
                    Ok(()) => {
                        view.reserved = next;
                        window.push_notification(
                            Notification::success(if next {
                                crate::i18n::t("reservation.done")
                            } else {
                                crate::i18n::t("reservation.cancelled")
                            }),
                            cx,
                        );
                    }
                    Err(e) => {
                        log::warn!("[Detail] 预约操作失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        window.push_notification(
                            Notification::error(crate::i18n::tf(
                                "common.failedWith",
                                &[("message", message.as_str())],
                            )),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

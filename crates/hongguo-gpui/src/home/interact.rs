//! 互动：集级公开计数、mget 最近互动、点赞/收藏/分享与右缘互动栏渲染。

use std::sync::Arc;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, Toggled, div};
use hongguo_api::interact::{InteractionItem, collect_series, digg_video, fetch_interaction_state};
use hongguo_common::AppError;

use super::NowPlaying;
use crate::bridge;

/// 互动栏 item 的展示参数（[`super::HomeView::rail_item`] 收拢散参用）。
struct RailItem {
    icon: gpui_kit::component::Icon,
    count: i64,
    active: bool,
    /// 激活态着色（红心/黄星）；未激活恒纯白（抖音系贴画面右缘形态）。
    active_color: gpui_kit::Rgba,
    /// 无计数项的文案（分享 → 「分享」）。
    label: Option<String>,
    /// 悬停 tooltip（悬停变手型 + 提示）。
    tooltip: String,
    /// 是否开关项（收藏/点赞/追更）：开关才带 aria-toggled；评论面板、分享
    /// 这类动作项没有开/关两态，带了就是假开关。
    toggleable: bool,
}

impl super::HomeView {
    /// 集级公开计数（剧集档案 detail 下发）：从本地档案读当前集的
    /// 评论数/点赞数（匿名可见的装饰性计数，阻塞池读 store）。
    pub(super) fn load_episode_counts(&mut self, now: &NowPlaying, cx: &mut Context<Self>) {
        let state = Arc::clone(&self.state);
        let (sid, vid_index) = (now.series_id.clone(), now.vid_index);
        self.ep_comment_count = 0;
        self.ep_digg_count = 0;
        let rx = bridge::spawn(async move {
            let counts = tokio::task::spawn_blocking(move || {
                state
                    .store
                    .series_by_id(&sid)
                    .ok()
                    .flatten()
                    .and_then(|s| s.episodes.into_iter().find(|e| e.vid_index == vid_index))
                    .map(|e| (e.comment_count, e.digg_count))
            })
            .await
            .map_err(|e| AppError::Internal(format!("集级计数阻塞池任务失败: {e}")))?;
            Ok::<_, AppError>(counts)
        });
        cx.spawn(async move |this, cx| {
            let counts = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, _| {
                match counts {
                    Ok(Some((comment_count, digg_count))) => {
                        // info 常驻：点赞数不显示类问题的第一现场（档案里有没有
                        // 这两个计数，一行钉死）
                        log::info!(
                            "[Home] 集级公开计数（{}#{}）：评论={} 点赞={}",
                            view.now
                                .as_ref()
                                .map(|n| n.series_id.as_str())
                                .unwrap_or("?"),
                            view.now.as_ref().map(|n| n.vid_index).unwrap_or(0),
                            comment_count,
                            digg_count
                        );
                        view.ep_comment_count = comment_count;
                        view.ep_digg_count = digg_count;
                    }
                    Ok(None) => log::warn!("[Home] 集级公开计数：档案里找不到当前集"),
                    Err(e) => log::warn!("[Home] 集级公开计数读取失败: {e}"),
                }
            });
        })
        .detach();
    }

    /// mget 最近互动（登录后才有「已赞/已追」红标；匿名/失败静默空表）。
    pub(super) fn load_interact_state(&mut self, cx: &mut Context<Self>) {
        if self.interact_loaded {
            return;
        }
        self.interact_loaded = true;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_interaction_state(&env)
                .await
                .map(|s| s.items)
                .unwrap_or_default()
        });
        cx.spawn(async move |this, cx| {
            let items = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, _| {
                view.interact_items = Arc::new(items);
            });
        })
        .detach();
    }

    /// 点赞当前集（裸 vid 为点赞对象）：乐观翻转 + 计数 ±1，失败回滚。
    fn toggle_digg(&mut self, cx: &mut Context<Self>) {
        if !self.logged_in() {
            self.require_login(cx);
            return;
        }
        let Some(now) = self.now.clone() else {
            return;
        };
        let bare_vid = now.vid.clone();
        if bare_vid.is_empty() || self.digg_pending {
            return;
        }
        // 当前态：mget 命中优先，否则按未赞
        let digged = self
            .interact_items
            .iter()
            .find(|i| i.vid == bare_vid)
            .map(|i| i.user_digg)
            .unwrap_or(false);
        let next = !digged;
        self.digg_pending = true;
        // 乐观：互动列表覆盖 + 计数 ±1
        let mut items = (*self.interact_items).clone();
        if let Some(hit) = items.iter_mut().find(|i| i.vid == bare_vid) {
            hit.user_digg = next;
            hit.digged_count += if next { 1 } else { -1 };
        } else {
            items.push(InteractionItem {
                vid: bare_vid.clone(),
                series_id: now.series_id.clone(),
                user_digg: next,
                digged_count: if next { 1 } else { 0 },
                followed: false,
                followed_cnt: 0,
                ..Default::default()
            });
        }
        self.interact_items = Arc::new(items);
        self.ep_digg_count += if next { 1 } else { -1 };
        let sid = now.series_id.clone();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            digg_video(&bare_vid, &sid, next, &env).await.map(|_| next)
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.digg_pending = false;
                match result {
                    Ok(_) => view.show_toast(
                        if next {
                            crate::i18n::t("player.interact.liked")
                        } else {
                            crate::i18n::t("player.interact.undone")
                        },
                        cx,
                    ),
                    Err(e) => {
                        // 失败回滚（下次重试）
                        log::warn!("[Home] 点赞失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        view.show_toast(
                            crate::i18n::tf("common.failedWith", &[("message", message.as_str())]),
                            cx,
                        );
                        view.load_interact_state(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 收藏/取消收藏当前剧（乐观翻转 + 收藏计数 ±1，失败回滚）。
    fn toggle_collect(&mut self, cx: &mut Context<Self>) {
        if !self.logged_in() {
            self.require_login(cx);
            return;
        }
        let Some(now) = self.now.clone() else {
            return;
        };
        if self.collect_pending {
            return;
        }
        let collected = self
            .interact_items
            .iter()
            .find(|i| i.series_id == now.series_id)
            .map(|i| i.followed)
            .unwrap_or(false);
        let next = !collected;
        self.collect_pending = true;
        let mut items = (*self.interact_items).clone();
        if let Some(hit) = items.iter_mut().find(|i| i.series_id == now.series_id) {
            hit.followed = next;
            hit.followed_cnt += if next { 1 } else { -1 };
        } else {
            items.push(InteractionItem {
                series_id: now.series_id.clone(),
                followed: next,
                followed_cnt: if next { 1 } else { 0 },
                ..Default::default()
            });
        }
        self.interact_items = Arc::new(items);
        self.follow_count += if next { 1 } else { -1 };
        let sid = now.series_id.clone();
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            collect_series(&sid, next, &env).await.map(|_| next)
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.collect_pending = false;
                match result {
                    Ok(_) => view.show_toast(
                        if next {
                            crate::i18n::t("player.interact.collected")
                        } else {
                            crate::i18n::t("player.interact.uncollected")
                        },
                        cx,
                    ),
                    Err(e) => {
                        log::warn!("[Home] 收藏失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        view.show_toast(
                            crate::i18n::tf("common.failedWith", &[("message", message.as_str())]),
                            cx,
                        );
                        view.load_interact_state(cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 分享：剧名 + 详情链接复制进剪贴板。
    fn share_series(&mut self, cx: &mut Context<Self>) {
        let Some(now) = &self.now else {
            return;
        };
        let text = format!(
            "{} https://hongguoduanju.com/detail?series_id={}",
            now.title, now.series_id
        );
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
        // Tauri 分享成功有 toast（player.interact.shareCopied）；这里也回执
        self.show_toast(crate::i18n::t("player.interact.shareCopied"), cx);
    }

    /// 互动登录墙（Tauri interaction-rail requireLogin 同款）：提示 + 去
    /// 设置页账号卡登录。切页走壳的「离开首页」语义（停播落进度，与 Tauri
    /// 离开播放页卸载同口径）。
    fn require_login(&mut self, cx: &mut Context<Self>) {
        log::info!("[Home] 互动需要登录（settings 里没有账号），跳设置页");
        self.show_toast(crate::i18n::t("player.interact.loginRequired"), cx);
        cx.emit(super::HomeEvent::OpenSettings);
    }

    /// 追更键显隐：**登录**且当前剧**连载中**（series_status=0）才出。
    /// 官方只在连载剧出追更钮（完结剧不会有新集，2026-10-15 实包同口径）；
    /// meta 未到（None）默认隐藏——完结剧在 meta 迟到/失败时不能挂着钮；
    /// 未登录整键不出现（用户实测官方口径：有登录才显示）。
    fn follow_visible(&self) -> bool {
        self.logged_in() && self.series_status == Some(0)
    }

    /// 追更当前剧（do_action 34/35，服务端跟随列表）：与详情页/追更页
    /// 同一段 service；乐观翻转 + 失败回滚，回执走舞台 toast。
    fn toggle_follow(&mut self, cx: &mut Context<Self>) {
        if self.follow_pending {
            return;
        }
        if !self.logged_in() {
            self.require_login(cx);
            return;
        }
        let Some(now) = self.now.clone() else {
            return;
        };
        let next = !self.follow_is_on.unwrap_or(false);
        self.follow_is_on = Some(next);
        self.follow_pending = true;
        let state = Arc::clone(&self.state);
        let sid = now.series_id.clone();
        let rx = bridge::spawn(async move {
            hongguo_service::follow_service::toggle(&state, &sid, next).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.follow_pending = false;
                match result {
                    Ok(()) => {
                        view.show_toast(
                            if next {
                                crate::i18n::t("follow.followed")
                            } else {
                                crate::i18n::t("follow.unfollowed")
                            },
                            cx,
                        );
                    }
                    Err(e) => {
                        // 失败回滚（下次重查拿服务端真相）
                        view.follow_is_on = Some(!next);
                        log::warn!("[Home] 追更失败: {e}");
                        let message = crate::i18n::app_error_text(&e);
                        view.show_toast(
                            crate::i18n::tf("common.failedWith", &[("message", message.as_str())]),
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

    /// 互动栏当前集/剧的命中（mget 列表里才有「已互动」态）。
    fn interact_hit(&self) -> (bool, bool) {
        let Some(now) = &self.now else {
            return (false, false);
        };
        let bare_vid = now.vid.split(':').next().unwrap_or("");
        let digged = self
            .interact_items
            .iter()
            .find(|i| i.vid == bare_vid)
            .map(|i| i.user_digg)
            .unwrap_or(false);
        let collected = self
            .interact_items
            .iter()
            .find(|i| i.series_id == now.series_id)
            .map(|i| i.followed)
            .unwrap_or(false);
        (digged, collected)
    }

    /// 互动栏右缘竖排（Tauri interaction-rail 对齐：收藏→评论→点赞→分享，
    /// icon 上计数下，激活态红心/黄星，显隐跟随控制条）。
    pub(super) fn render_interaction_rail(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let (digged, collected) = self.interact_hit();
        // 计数优先档案公开计数（匿名可见）；mget 命中值兜底
        let digg_count = if self.ep_digg_count > 0 {
            self.ep_digg_count
        } else {
            self.interact_items
                .iter()
                .find(|i| i.vid == self.now.as_ref().map(|n| n.vid.clone()).unwrap_or_default())
                .map(|i| i.digged_count)
                .unwrap_or(0)
        };
        let collect_count = if self.follow_count > 0 {
            self.follow_count
        } else {
            self.interact_items
                .iter()
                .find(|i| {
                    Some(i.series_id.as_str()) == self.now.as_ref().map(|n| n.series_id.as_str())
                })
                .map(|i| i.followed_cnt)
                .unwrap_or(0)
        };
        let comment_count = self.ep_comment_count;

        // 单个互动项：icon 上、计数下，激活态换色（抖音系右栏单元）。
        // helper 收 cx.listener 的产物即可，避免闭包里再借 cx。
        // 红色是播放器皮肤（硬红线 11 的播放器豁免）。
        v_flex()
            .absolute()
            .right_2()
            .bottom_24()
            .items_center()
            .gap_4()
            // 滚轮拦下（Tauri interaction-rail data-wheel-block 同款）：
            // 悬在互动键上滚不切剧
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(self.rail_item(
                "collect",
                RailItem {
                    icon: gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Star),
                    count: collect_count,
                    active: collected,
                    active_color: gpui_kit::rgba(0xfacc15ff),
                    label: None,
                    tooltip: crate::i18n::t("player.interact.collect"),
                    toggleable: true,
                },
                cx,
                cx.listener(|this, _, _, cx| this.toggle_collect(cx)),
            ))
            .child(self.rail_item(
                "comments",
                RailItem {
                    icon: gpui_kit::component::Icon::new(
                        gpui_kit::assets::IconName::MessageCircleMore,
                    ),
                    count: comment_count,
                    active: false,
                    active_color: gpui_kit::rgba(0xffffffff),
                    label: None,
                    tooltip: crate::i18n::t("player.interact.comments"),
                    toggleable: false,
                },
                cx,
                cx.listener(|this, _, _, cx| this.toggle_comments_panel(cx)),
            ))
            .child(self.rail_item(
                "digg",
                RailItem {
                    icon: gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Heart),
                    count: digg_count,
                    active: digged,
                    active_color: gpui_kit::rgba(0xef4444ff),
                    label: None,
                    tooltip: crate::i18n::t("player.interact.like"),
                    toggleable: true,
                },
                cx,
                cx.listener(|this, _, _, cx| this.toggle_digg(cx)),
            ))
            // 追更（📺，仅连载中显示——已完结剧不会有新集；Tauri 同位）
            .children(self.follow_visible().then(|| {
                let follow_on = self.follow_is_on.unwrap_or(false);
                self.rail_item(
                    "follow",
                    RailItem {
                        icon: gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Tv),
                        count: 0,
                        active: follow_on,
                        // Tauri 追更态用 text-primary（语义令牌，非皮肤色）
                        active_color: cx.theme().primary.to_rgb(),
                        label: Some(if follow_on {
                            crate::i18n::t("follow.following")
                        } else {
                            crate::i18n::t("follow.follow")
                        }),
                        tooltip: crate::i18n::t("player.interact.follow"),
                        toggleable: true,
                    },
                    cx,
                    cx.listener(|this, _, _, cx| this.toggle_follow(cx)),
                )
            }))
            .child(self.rail_item(
                "share",
                RailItem {
                    icon: gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Forward),
                    count: 0,
                    active: false,
                    active_color: gpui_kit::rgba(0xffffffff),
                    label: Some(crate::i18n::t("player.interact.share")),
                    tooltip: crate::i18n::t("player.interact.share"),
                    toggleable: false,
                },
                cx,
                cx.listener(|this, _, _, cx| this.share_series(cx)),
            ))
            .into_any_element()
    }

    /// 互动栏单个 item（icon 上、计数下；`label` 给无计数项的文案——分享
    /// 显示「分享」二字，Tauri 同款）。
    fn rail_item(
        &self,
        id: &'static str,
        item: RailItem,
        cx: &Context<Self>,
        on_click: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
    ) -> gpui_kit::AnyElement {
        let RailItem {
            icon,
            count,
            active,
            active_color,
            label,
            tooltip,
            toggleable,
        } = item;
        let tooltip = tooltip.to_string();
        // 可点 → base::Button：焦点句柄、Tab 可达、Enter/Space 激活与 role
        // 由库负责；无障碍名用该项的 tooltip（每项都有）。
        BaseButton::new(ElementId::from(format!("rail-{id}")))
            .accessibility_label(tooltip.clone())
            .when(toggleable, |s| {
                s.aria_toggled(if active {
                    Toggled::True
                } else {
                    Toggled::False
                })
            })
            .flex()
            .flex_col()
            .items_center()
            .gap_0p5()
            .cursor_pointer()
            .text_color(if active {
                active_color
            } else {
                gpui_kit::rgb(0xffffff)
            })
            // base Button 强制 justify_center：原链是 flex column 且没写
            // justify，内容从顶部排，钉回顶部对齐。
            .justify_start()
            // base Button 强制 line_height(relative(1.))：计数/文案子元素继承
            // 行高，钉回默认行高才与原 div 像素一致。
            .line_height(gpui_kit::phi())
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
            })
            // 互动栏悬浮在画面之上，而 gpui 对重叠 bounds 的兄弟分支都会派发
            // 鼠标事件——不拦的话点互动键会同时触发「点画面暂停」
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .focus_visible(|s| s.shadow(crate::home::skin_focus::skin_focus_ring()))
            .on_click(on_click)
            .child(icon.size_7().text_color(if active {
                active_color
            } else {
                gpui_kit::rgb(0xffffff)
            }))
            .children((count > 0).then(|| {
                div()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .text_color(gpui_kit::rgb(0xffffff))
                    .child(Self::fmt_count(count))
            }))
            .children(label.map(|t| {
                div()
                    .text_xs()
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .text_color(gpui_kit::rgb(0xffffff))
                    .child(t)
            }))
            .into_any_element()
    }
}

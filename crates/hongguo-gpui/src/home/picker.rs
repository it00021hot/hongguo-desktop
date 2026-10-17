//! 选集浮层：数字网格选集、头部双 tab、「系列剧」tab（相关作品·系列拉取
//! 与预约）与浮层开关。

use std::sync::Arc;
#[cfg(feature = "screenshot")]
use std::time::{Duration, Instant};

use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, SharedString, div, img};
use hongguo_api::detail::{RelatedItem, fetch_related_series};
use hongguo_api::reservation::{fetch_reservations, reserve_series};

#[cfg(feature = "screenshot")]
use super::controls::CONTROLS_HOLD_SECS;
use super::feed::StreamItem;
use crate::bridge;

impl super::HomeView {
    /// 拉当前剧的相关作品·系列（plan/v；每剧一次，失败静默可重试）。
    /// 季卡片数据源：works 里季号可解析的条目（当前剧由浮层渲染时从
    /// 档案合成插头部，与 Tauri seasons memo 同构）。
    pub(super) fn load_related_seasons(&mut self, series_id: String, cx: &mut Context<Self>) {
        if self.related_seasons.contains_key(&series_id)
            || !self.related_loading.insert(series_id.clone())
        {
            return;
        }
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_related_series(&sid, &env)
                .await
                .map(|r| r.works)
                .map_err(|_| sid)
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| match result {
                Ok(works) => {
                    // 季卡片封面走同一张 covers 表（懒加载，与全屏占位同源）
                    for item in &works {
                        view.load_cover(item.series_id.clone(), item.cover.clone(), cx);
                    }
                    view.related_seasons
                        .insert(series_id.clone(), Arc::new(works));
                }
                Err(sid) => {
                    view.related_loading.remove(&sid);
                }
            });
        })
        .detach();
    }

    /// 季列表合成（Tauri seasons memo 同构）：plan 只回其他季，当前季
    /// 缺失时从档案合成插头部；按季号排序（服务端顺序不可信）。
    fn seasons_of(&self) -> (String, Vec<(RelatedItem, u32)>) {
        let Some(now) = &self.now else {
            return (String::new(), Vec::new());
        };
        let sid = &now.series_id;
        let works = self
            .related_seasons
            .get(sid)
            .map(|v| (**v).clone())
            .unwrap_or_default();
        let mut out: Vec<(RelatedItem, u32)> = works
            .iter()
            .filter_map(|w| {
                crate::home::season::season_no_from_tag(&w.tag).map(|no| (w.clone(), no))
            })
            .collect();
        if !out.iter().any(|(w, _)| w.series_id == *sid) {
            // 当前季缺失：从档案合成（tag/playCnt 档案没有，卡片留空即可）
            out.push((
                RelatedItem {
                    series_id: sid.clone(),
                    title: now.title.clone(),
                    tag: String::new(),
                    episode_cnt: now.episode_count as u32,
                    ..Default::default()
                },
                0,
            ));
        }
        out.sort_by_key(|(_, no)| *no);
        (sid.clone(), out)
    }

    /// 点季卡片换剧（点当前季无动作；关闭浮层交给用户，连选同款不强制）。
    fn switch_season(&mut self, series_id: String, cx: &mut Context<Self>) {
        let Some(item) = self
            .display_items()
            .iter()
            .find(|i| i.series_id == series_id)
            .cloned()
        else {
            // 流里没有的季（跨剧换季）：构造目标条目直接起播
            self.play_target(
                StreamItem {
                    series_id,
                    ..Default::default()
                },
                None,
                None,
                cx,
            );
            cx.notify();
            return;
        };
        self.play_target(item, None, None, cx);
        cx.notify();
    }

    /// 开关选集浮层（连选连跳：选中不关，关闭走 Esc/收起/点外部）。
    pub(super) fn toggle_picker(&mut self, cx: &mut Context<Self>) {
        self.picker_open = !self.picker_open;
        // 重开跟回当前集所在段（覆盖作废，Tauri groupOverride 同语义）
        self.picker_group_override = None;
        self.wake_controls(cx);
        // 系列剧 tab 数据源：浮层打开时按剧拉一次
        if self.picker_open
            && let Some(now) = self.now.clone()
        {
            self.load_related_seasons(now.series_id.clone(), cx);
            self.load_reservations(cx);
        }
        cx.notify();
    }

    /// 选集浮层（Tauri EpisodePicker 对齐）：贴控制栏向上弹的数字网格。
    /// 范围段每段 30 集（hgplayer 同款）；当前集红底高亮；点选**不关**浮层
    /// （连选连跳，关闭走 Esc/收起/点外部）。红色是 hgplayer 皮肤（硬红线
    /// 11 的播放器豁免）。「系列剧」tab 与下载完成角标随相关作品/预约、
    /// 下载能力的对齐步骤补。
    pub(super) fn render_picker(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let Some(now) = &self.now else {
            return div().into_any_element();
        };
        let total = now.episode_count as u32;
        const GROUP_SIZE: u32 = 30;
        let group_count = total.div_ceil(GROUP_SIZE).max(1) as usize;
        let follow_group =
            (now.vid_index.saturating_sub(1) as usize / GROUP_SIZE as usize).min(group_count - 1);
        let active_group = match self.picker_group_override {
            Some((group, at)) if at == now.vid_index => group,
            _ => follow_group,
        };
        let start = active_group as u32 * GROUP_SIZE + 1;
        let end = (start + GROUP_SIZE - 1).min(total);
        // 系列剧 tab 的计数角标（Tauri `seasons.length` 同款：浮层打开时
        // 系列列表已拉过，非空才显示）
        let season_count = self.seasons_of().1.len();

        // 头部：选集 | 系列剧 双 tab（hgplayer 同款）+ 全 N 集 + 收起
        let header = h_flex()
            .items_center()
            .justify_between()
            .mb_3()
            .child(
                h_flex()
                    .gap_4()
                    .child(self.picker_tab(
                        crate::i18n::t("player.episodes"),
                        !self.picker_tab_series,
                        None,
                        cx.listener(|this, _, _, cx| {
                            this.picker_tab_series = false;
                            cx.notify();
                        }),
                    ))
                    .child(self.picker_tab(
                        crate::i18n::t("player.seriesList"),
                        self.picker_tab_series,
                        (season_count > 0).then_some(season_count),
                        cx.listener(|this, _, _, cx| {
                            this.picker_tab_series = true;
                            if let Some(now) = this.now.clone() {
                                this.load_related_seasons(now.series_id.clone(), cx);
                            }
                            cx.notify();
                        }),
                    )),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .children((!self.picker_tab_series).then(|| {
                        div()
                            .text_xs()
                            .text_color(gpui_kit::rgba(0xa3a3a3ff))
                            .child(crate::i18n::tf(
                                "player.totalEpisodes",
                                &[("count", &total.to_string())],
                            ))
                    }))
                    .child(
                        Button::new("picker-close")
                            .icon(IconName::ChevronDown)
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.picker_open = false;
                                this.picker_group_override = None;
                                cx.notify();
                            })),
                    ),
            );

        // 范围段 tab（1-30 / 31-60 / …；只有一段时不渲染）
        let group_tabs = div()
            .flex()
            .flex_wrap()
            .gap_x_4()
            .gap_y_1()
            .mb_3()
            .children((0..group_count).map(|i| {
                let active = i == active_group;
                let lo = i as u32 * GROUP_SIZE + 1;
                let hi = ((i as u32 + 1) * GROUP_SIZE).min(total);
                div()
                    .id(gpui_kit::ElementId::from(format!("picker-group-{i}")))
                    .cursor_pointer()
                    .text_size(gpui_kit::px(13.0))
                    .text_color(if active {
                        gpui_kit::rgb(0xffffff)
                    } else {
                        gpui_kit::rgba(0xa3a3a3ff)
                    })
                    .hover(|s| s.text_color(gpui_kit::rgba(0xe5e5e5ff)))
                    .when(active, |s| s.font_weight(gpui_kit::FontWeight::SEMIBOLD))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let at = this.now.as_ref().map(|n| n.vid_index).unwrap_or(0);
                        this.picker_group_override = Some((i, at));
                        cx.notify();
                    }))
                    .child(format!("{lo}-{hi}"))
            }));

        // 8 列数字网格（右上角下载完成角标：绿点 = 该集已下载可离线播）
        let grid = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .children((start..=end).map(|n| {
                let active = n == now.vid_index;
                let downloaded = self.done_vids.contains(&n);
                div()
                    .id(gpui_kit::ElementId::from(format!("picker-ep-{n}")))
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(gpui_kit::px(48.0))
                    .h(gpui_kit::px(36.0))
                    .rounded(gpui_kit::px(6.0))
                    .text_sm()
                    .cursor_pointer()
                    .text_color(if active {
                        gpui_kit::rgb(0xffffff)
                    } else {
                        gpui_kit::rgba(0xe5e5e5ff)
                    })
                    .bg(if active {
                        gpui_kit::rgba(0xef4444ff)
                    } else {
                        gpui_kit::rgba(0x262626cc)
                    })
                    .hover(|s| s.bg(gpui_kit::rgba(0x404040ff)))
                    .when(active, |s| s.font_weight(gpui_kit::FontWeight::SEMIBOLD))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_episode(n, cx)))
                    .child(format!("{n}"))
                    .children(downloaded.then(|| {
                        div()
                            .absolute()
                            .top_1()
                            .right_1()
                            .size_1p5()
                            .rounded_full()
                            .bg(if active {
                                gpui_kit::rgba(0xfffffffa)
                            } else {
                                gpui_kit::rgba(0x34d399ff)
                            })
                    }))
            }));

        v_flex()
            .id("picker-panel")
            .absolute()
            // 贴「选集」按钮向上弹（Tauri EpisodePicker 挂按钮 wrapper 同款：
            // 右缘与按钮对齐、间距 8）
            .right_0()
            .bottom(gpui_kit::relative(1.0))
            .mb(gpui_kit::px(8.0))
            .w(gpui_kit::px(452.0))
            .max_h(gpui_kit::px(440.0))
            .p_4()
            .rounded(gpui_kit::px(12.0))
            .bg(gpui_kit::rgba(0x171717f2))
            .overflow_y_scroll()
            // 浮层内的任何点击都不许落到捕获层/舞台（点选集把视频点停了
            // 就是这里漏的——Tauri EpisodePicker 根节点 onClick 同款）
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .on_mouse_up(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            // 滚轮拦下（Tauri seriesPanelOpen 守卫同语义）：浮层列表自己在
            // 滚，不冒泡成舞台切剧
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(header)
            .children(if self.picker_tab_series {
                Some(self.render_seasons_tab(cx))
            } else {
                (group_count > 1)
                    .then_some(group_tabs)
                    .map(|tabs| div().child(tabs).child(grid).into_any_element())
            })
            .into_any_element()
    }

    /// 选集浮层头部的一个 tab（红条高亮激活态）。
    fn picker_tab(
        &self,
        label: impl Into<SharedString>,
        active: bool,
        badge: Option<usize>,
        on_click: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
    ) -> gpui_kit::AnyElement {
        let label: SharedString = label.into();
        div()
            .id(ElementId::from(format!("picker-tab-{label}")))
            .relative()
            .pb_1()
            .text_sm()
            .cursor_pointer()
            .font_weight(if active {
                gpui_kit::FontWeight::SEMIBOLD
            } else {
                gpui_kit::FontWeight::NORMAL
            })
            .text_color(if active {
                gpui_kit::rgb(0xffffff)
            } else {
                gpui_kit::rgba(0xa3a3a3ff)
            })
            .hover(|s| s.text_color(gpui_kit::rgba(0xe5e5e5ff)))
            .when(active, |s| {
                s.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(gpui_kit::px(2.0))
                        .rounded_full()
                        .bg(gpui_kit::rgb(0xef4444)),
                )
            })
            .on_click(on_click)
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .child(label)
                    .children(badge.map(|n| {
                        // Tauri Badge：bg-red-500 圆角胶囊、白字
                        div()
                            .rounded_full()
                            .bg(gpui_kit::rgb(0xef4444))
                            .px_1p5()
                            .text_size(gpui_kit::px(11.0))
                            .text_color(gpui_kit::rgb(0xffffff))
                            .child(SharedString::from(n.to_string()))
                    })),
            )
            .into_any_element()
    }

    /// 拉离线预约列表（会话级一次；系列季 tab「已预约」态的数据源，
    /// Tauri useReservations(false) 同口径）。
    fn load_reservations(&mut self, cx: &mut Context<Self>) {
        if self.reservations_loaded || !self.logged_in() {
            return;
        }
        self.reservations_loaded = true;
        let state = Arc::clone(&self.state);
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            fetch_reservations(false, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            if let Ok(page) = result {
                let _ = this.update(cx, |view, cx| {
                    for item in page.items {
                        view.reserved_ids.insert(item.series_id);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 预约一个未上线季（乐观置「已预约」，失败回滚；Tauri useReserveSeries
    /// 同口径——只做预约，取消预约在日历页，浮层里没有入口）。
    fn reserve_season(&mut self, series_id: String, cx: &mut Context<Self>) {
        if !self.logged_in() {
            self.show_toast(crate::i18n::t("player.interact.loginRequired"), cx);
            return;
        }
        if self.reserved_ids.contains(&series_id) || !self.reserve_pending.insert(series_id.clone())
        {
            return;
        }
        self.reserved_ids.insert(series_id.clone());
        cx.notify();
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            reserve_series(&sid, true, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update(cx, |view, cx| {
                view.reserve_pending.remove(&series_id);
                match result {
                    Ok(()) => view.show_toast(crate::i18n::t("reservation.done"), cx),
                    Err(e) => {
                        view.reserved_ids.remove(&series_id);
                        log::warn!("[Home] 预约失败: {e}");
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
    }

    /// 系列剧 tab：各季卡片（Tauri EpisodePicker 同款：封面 + 剧名 +
    /// 「tag · 全N集 · 播放量」数据行，右侧 播放中/预约/播放 三态；当前季
    /// 红框高亮，可播季点击换剧，浮层不关）。封面走 covers 表（HEIC 签名
    /// URL 经 cover 管线，拉取在 load_related_seasons 完成处发起）。
    fn render_seasons_tab(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let (current_sid, seasons) = self.seasons_of();
        v_flex()
            .gap_2()
            .children(seasons.iter().map(|(item, no)| {
                let current = item.series_id == current_sid;
                let unreleased = item.episode_cnt == 0;
                let border_color = if current {
                    gpui_kit::rgba(0xef4444cc)
                } else {
                    gpui_kit::rgba(0x00000000)
                };
                let sid_click = item.series_id.clone();
                let cover = self.covers.get(&item.series_id).cloned();
                let mut meta: Vec<String> = Vec::new();
                if !item.tag.is_empty() {
                    meta.push(item.tag.clone());
                }
                if item.episode_cnt > 0 {
                    meta.push(crate::i18n::tf(
                        "player.totalEpisodes",
                        &[("count", &item.episode_cnt.to_string())],
                    ));
                }
                if item.play_cnt > 0 {
                    meta.push(crate::i18n::tf(
                        "browse.playCount",
                        &[("count", &Self::fmt_play_count(item.play_cnt))],
                    ));
                }
                h_flex()
                    .id(gpui_kit::ElementId::from(format!(
                        "season-{}",
                        item.series_id
                    )))
                    .items_center()
                    .gap_2p5()
                    .p_2p5()
                    .rounded(gpui_kit::px(8.0))
                    .border_1()
                    .border_color(border_color)
                    .bg(gpui_kit::rgba(0x26262666))
                    .when(!current && !unreleased, |s| {
                        s.cursor_pointer()
                            .hover(|s| s.bg(gpui_kit::rgba(0x333333cc)))
                    })
                    .when(!current && !unreleased, |s| {
                        s.on_click(cx.listener(move |this, _, _, cx| {
                            // 换季 = 换剧（播放器直接起播该季第 1 集），浮层不关
                            // ——与选集 tab 的连选连跳同款
                            this.switch_season(sid_click.clone(), cx);
                        }))
                    })
                    .child(
                        // 封面（竖版 40×56；未就绪给底色占位，到位即换）
                        div()
                            .h(gpui_kit::px(56.0))
                            .w(gpui_kit::px(40.0))
                            .flex_shrink_0()
                            .overflow_hidden()
                            .rounded(gpui_kit::px(6.0))
                            .bg(gpui_kit::rgba(0x333333cc))
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
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .text_color(gpui_kit::rgb(0xffffff))
                                    .truncate()
                                    .child(SharedString::from(if item.title.is_empty() {
                                        crate::i18n::tf(
                                            "player.seasonNo",
                                            &[("no", &no.to_string())],
                                        )
                                    } else {
                                        item.title.clone()
                                    })),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .children(unreleased.then(|| {
                                        div()
                                            .px_1()
                                            .rounded(gpui_kit::px(2.0))
                                            .bg(gpui_kit::rgba(0xef4444e6))
                                            .text_size(gpui_kit::px(10.0))
                                            .line_height(gpui_kit::relative(1.45))
                                            .text_color(gpui_kit::rgb(0xffffff))
                                            .child(crate::i18n::t("player.comingSoon"))
                                    }))
                                    .child(
                                        div()
                                            .text_size(gpui_kit::px(11.0))
                                            .text_color(gpui_kit::rgba(0xa3a3a3ff))
                                            .truncate()
                                            .child(SharedString::from(meta.join(" · "))),
                                    ),
                            ),
                    )
                    .children(current.then(|| {
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .text_color(gpui_kit::rgb(0xef4444))
                            .child(crate::i18n::t("player.nowPlaying"))
                    }))
                    .children((unreleased && !current).then(|| {
                        // 未上线季：预约按钮（Tauri BellRing 圆胶囊同款；
                        // 已预约置灰禁点，预约在途防重复）
                        let sid = item.series_id.clone();
                        let reserved = self.reserved_ids.contains(&item.series_id);
                        let pending = self.reserve_pending.contains(&item.series_id);
                        div()
                            .id(gpui_kit::ElementId::from(format!(
                                "season-reserve-{}",
                                item.series_id
                            )))
                            .h_7()
                            .px_3()
                            .rounded_full()
                            .border_1()
                            .border_color(if reserved {
                                gpui_kit::rgba(0xffffff26)
                            } else {
                                gpui_kit::rgba(0x525252ff)
                            })
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_xs()
                            .text_color(if reserved {
                                gpui_kit::rgba(0xffffff73)
                            } else {
                                gpui_kit::rgb(0xffffff)
                            })
                            .when(!reserved && !pending, |s| {
                                s.cursor_pointer()
                                    .hover(|s| s.bg(gpui_kit::rgba(0x333333cc)))
                            })
                            .when(!reserved && !pending, |s| {
                                s.on_click(cx.listener(move |this, _, _, cx| {
                                    this.reserve_season(sid.clone(), cx);
                                }))
                            })
                            .child(Icon::new(gpui_kit::assets::IconName::BellRing).size_3())
                            .child(if reserved {
                                crate::i18n::t("reservation.reserved")
                            } else if pending {
                                crate::i18n::t("reservation.reserving")
                            } else {
                                crate::i18n::t("reservation.action")
                            })
                    }))
                    .children((!current && !unreleased).then(|| {
                        // 可播季：播放胶囊（Tauri Play 圆角描边胶囊同款）
                        h_flex()
                            .flex_shrink_0()
                            .h_7()
                            .px_3()
                            .rounded_full()
                            .border_1()
                            .border_color(gpui_kit::rgba(0x525252ff))
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(gpui_kit::rgba(0xd4d4d4ff))
                            .child(Icon::new(gpui_kit::assets::IconName::Play).size_3())
                            .child(crate::i18n::t("player.play"))
                    }))
            }))
            .into_any_element()
    }

    /// 取证辅助（screenshot feature 的 HONGGUO_SHOT_PANEL=picker）：打开
    /// 选集浮层的系列剧 tab 并装载数据（无人值守抓季卡片/预约按钮）。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_open_picker(&mut self, cx: &mut Context<Self>) {
        self.picker_open = true;
        // 默认系列剧 tab（验季卡片/预约）；HONGGUO_PICKER_TAB=grid 拍选集网格
        self.picker_tab_series = std::env::var("HONGGUO_PICKER_TAB")
            .map(|v| v != "grid")
            .unwrap_or(true);
        self.controls_until = Some(Instant::now() + Duration::from_secs(CONTROLS_HOLD_SECS));
        if let Some(now) = self.now.clone() {
            self.load_related_seasons(now.series_id.clone(), cx);
            self.load_reservations(cx);
        }
        cx.notify();
    }
}

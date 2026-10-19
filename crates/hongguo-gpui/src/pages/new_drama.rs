//! 新剧页（Tauri `/new` = `new-drama-page.tsx` 的 GPUI 对齐）。
//!
//! 两个视图：新剧推荐（`firstonlinetime_new` 倒序流，频道筛选 全部/男频/
//! 女频，封面网格 + 翻页）与上新日历（日期条 + 当日上新行卡，未上线可
//! 预约）。视图 tab 按壳约定放主体区顶部（Tauri 在顶栏 portal 同位）。
//! 点击分流：未上线进详情（带 prefill 走「即将上线」降级视图），已上线
//! 直接起播。数据走 `hongguo_api::{new_drama, calendar}`，与 Tauri 的
//! new_drama 系列 command 同一段。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, RenderImage, SharedString, Window, div, img};
use hongguo_api::calendar::CalendarItem;
use hongguo_api::rank::RankItem;
use hongguo_service::app_state::AppStateInner;

use crate::bridge;
use crate::detail::DetailPrefill;

/// 新剧页发往壳的事件（起播 / 带 prefill 进详情）。
#[derive(Debug, Clone)]
pub(crate) enum NewDramaEvent {
    Play {
        series_id: String,
        vid_index: u32,
        start_secs: f64,
    },
    OpenDetail {
        series_id: String,
        prefill: DetailPrefill,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewTab {
    Recommend,
    Calendar,
}

pub(crate) struct NewDramaView {
    state: Arc<AppStateInner>,
    view: ViewTab,
    gender: i64,
    // —— 推荐流 ——
    items: Vec<RankItem>,
    next_offset: i64,
    session_id: String,
    has_more: bool,
    loading: bool,
    failed: bool,
    // —— 日历 ——
    cal_dates: Vec<String>,
    cal_selected: String,
    cal_items: Vec<CalendarItem>,
    cal_next_offset: i64,
    cal_session_id: String,
    cal_has_more: bool,
    cal_loading: bool,
    cal_failed: bool,
    cal_loaded: bool,
    /// 日历本地预约态（日历响应 has_subscribed 恒 false，本地记已点过的剧）。
    reserved_local: HashSet<String>,
    covers: HashMap<String, Arc<RenderImage>>,
    covers_pending: HashSet<String>,
    downloaded: HashMap<String, u32>,
}

impl NewDramaView {
    pub(crate) fn new(state: Arc<AppStateInner>) -> Self {
        // 取证开关（HONGGUO_NEW_TAB=calendar）：直接落在上新日历视图
        let start_calendar = std::env::var("HONGGUO_NEW_TAB").is_ok_and(|v| v == "calendar");
        Self {
            state,
            view: if start_calendar {
                ViewTab::Calendar
            } else {
                ViewTab::Recommend
            },
            gender: 2,
            items: Vec::new(),
            next_offset: 0,
            session_id: String::new(),
            has_more: false,
            loading: false,
            failed: false,
            cal_dates: Vec::new(),
            cal_selected: String::new(),
            cal_items: Vec::new(),
            cal_next_offset: 0,
            cal_session_id: String::new(),
            cal_has_more: false,
            cal_loading: false,
            cal_failed: false,
            cal_loaded: false,
            reserved_local: HashSet::new(),
            covers: HashMap::new(),
            covers_pending: HashSet::new(),
            downloaded: HashMap::new(),
        }
    }

    pub(crate) fn load(&mut self, cx: &mut Context<Self>) {
        self.refresh_downloaded();
        if self.view == ViewTab::Recommend && self.items.is_empty() && !self.loading {
            self.load_recommend(false, cx);
        } else if self.view == ViewTab::Calendar && !self.cal_loaded {
            self.load_calendar(None, false, cx);
        }
    }

    fn refresh_downloaded(&mut self) {
        let mut map: HashMap<String, u32> = HashMap::new();
        for task in self.state.queue().all() {
            if task.status == hongguo_common::model::TaskStatus::Completed {
                *map.entry(task.series_id).or_insert(0) += 1;
            }
        }
        self.downloaded = map;
    }

    /// 拉一页推荐（`more` = 翻页）。
    fn load_recommend(&mut self, more: bool, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.failed = false;
        cx.notify();
        let state = Arc::clone(&self.state);
        let gender = self.gender;
        let offset = if more { self.next_offset } else { 0 };
        let session = if more {
            self.session_id.clone()
        } else {
            String::new()
        };
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::new_drama::fetch_new_drama(
                gender,
                offset,
                (!session.is_empty()).then_some(session.as_str()),
                &env,
            )
            .await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.loading = false;
                match result {
                    Ok(page) => {
                        log::info!("[New] GPUI 壳 -> {} 条", page.items.len());
                        if more {
                            view.items.extend(page.items.clone());
                        } else {
                            view.items = page.items.clone();
                        }
                        view.next_offset = page.next_offset;
                        view.session_id = page.session_id.clone();
                        view.has_more = page.has_more;
                        let urls: Vec<String> =
                            page.items.iter().map(|i| i.cover.clone()).collect();
                        view.load_covers(&urls, cx);
                    }
                    Err(e) => {
                        log::warn!("[New] 推荐流失败: {e}");
                        view.failed = true;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 切频道：整页重来（Tauri 换 key 重挂载同口径）。
    fn switch_gender(&mut self, gender: i64, cx: &mut Context<Self>) {
        if self.gender == gender {
            return;
        }
        self.gender = gender;
        self.items.clear();
        self.next_offset = 0;
        self.session_id.clear();
        self.has_more = false;
        self.load_recommend(false, cx);
        cx.notify();
    }

    /// 拉一页日历（`date` 为空 = 默认日；`more` = 当日续页）。
    fn load_calendar(&mut self, date: Option<String>, more: bool, cx: &mut Context<Self>) {
        if self.cal_loading {
            return;
        }
        self.cal_loading = true;
        self.cal_failed = false;
        cx.notify();
        let state = Arc::clone(&self.state);
        let offset = if more { self.cal_next_offset } else { 0 };
        let session = if more {
            self.cal_session_id.clone()
        } else {
            String::new()
        };
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            let cursor = (offset > 0).then_some((offset, session.as_str()));
            hongguo_api::calendar::fetch_new_calendar(date.as_deref(), cursor, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            this.update(cx, |view, cx| {
                view.cal_loading = false;
                view.cal_loaded = true;
                match result {
                    Ok(page) => {
                        log::info!("[New] 日历 -> {} 条", page.items.len());
                        if !more {
                            view.cal_items = page.items.clone();
                            if !page.dates.is_empty() {
                                view.cal_dates = page.dates.clone();
                            }
                            if view.cal_selected.is_empty() {
                                view.cal_selected = if page.default_date.is_empty() {
                                    view.cal_dates
                                        .get(1)
                                        .or_else(|| view.cal_dates.first())
                                        .cloned()
                                        .unwrap_or_default()
                                } else {
                                    page.default_date.clone()
                                };
                            }
                        } else {
                            view.cal_items.extend(page.items.clone());
                        }
                        view.cal_next_offset = page.next_offset;
                        view.cal_session_id = page.session_id.clone();
                        view.cal_has_more = page.has_more;
                        let urls: Vec<String> =
                            page.items.iter().map(|i| i.cover.clone()).collect();
                        view.load_covers(&urls, cx);
                        // 混排页过滤后暂空但还有下一页：自动续拉（Tauri 同款）
                        let selected = view.cal_selected.clone();
                        let visible = view
                            .cal_items
                            .iter()
                            .filter(|i| selected.is_empty() || selected_date_of(i) == selected)
                            .count();
                        if visible == 0 && page.has_more && !more {
                            view.load_calendar(None, true, cx);
                        }
                    }
                    Err(e) => {
                        log::warn!("[New] 日历失败: {e}");
                        view.cal_failed = true;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_covers(&mut self, urls: &[String], cx: &mut Context<Self>) {
        for url in urls {
            let url = url.clone();
            if url.is_empty() || !self.covers_pending.insert(url.clone()) {
                continue;
            }
            let rx = crate::cover_img::fetch(url.clone());
            cx.spawn(async move |this, cx| {
                let bytes = rx.await.expect("tokio 桥不会丢弃发送端");
                this.update(cx, |view, cx| {
                    view.covers_pending.remove(&url);
                    if let Ok(bytes) = bytes
                        && let Ok(decoded) = image::load_from_memory(&bytes)
                    {
                        view.covers.insert(
                            url.clone(),
                            Arc::new(crate::image_upload::render_image(decoded.to_rgba8())),
                        );
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    fn play_series(&self, series_id: String, cx: &mut Context<Self>) {
        let (vid_index, start_secs) = hongguo_service::play_service::resume::resolve_entry(
            &self.state.store,
            &series_id,
            &[],
        )
        .ok()
        .unwrap_or((1, 0.0));
        cx.emit(NewDramaEvent::Play {
            series_id,
            vid_index,
            start_secs,
        });
    }

    fn open_detail(&self, series_id: String, prefill: DetailPrefill, cx: &mut Context<Self>) {
        cx.emit(NewDramaEvent::OpenDetail { series_id, prefill });
    }

    /// 推荐卡点击分流（Tauri handleSelect 同口径）。
    fn select_rank_item(&self, item: &RankItem, cx: &mut Context<Self>) {
        if item.upcoming {
            self.open_detail(
                item.series_id.clone(),
                DetailPrefill {
                    title: item.title.clone(),
                    cover: item.cover.clone(),
                    tags: item.tags.clone(),
                    desc: item.description.clone(),
                    reserved: item.reserved,
                },
                cx,
            );
        } else {
            self.play_series(item.series_id.clone(), cx);
        }
    }

    /// 日历卡预约（未上线行尾动作）。
    fn reserve(&mut self, series_id: String, cx: &mut Context<Self>) {
        if self.reserved_local.contains(&series_id) {
            return;
        }
        let state = Arc::clone(&self.state);
        let sid = series_id.clone();
        let rx = bridge::spawn(async move {
            let env = state.api_env();
            hongguo_api::reservation::reserve_series(&sid, true, &env).await
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.expect("tokio 桥不会丢弃发送端");
            let _ = this.update_in(cx, |view, window, cx| match result {
                Ok(()) => {
                    view.reserved_local.insert(series_id);
                    window.push_notification(
                        gpui_kit::component::notification::Notification::success(crate::i18n::t(
                            "reservation.done",
                        )),
                        cx,
                    );
                    cx.notify();
                }
                Err(e) => {
                    log::warn!("[New] 预约失败: {e}");
                    window.push_notification(
                        gpui_kit::component::notification::Notification::error(crate::i18n::tf(
                            "common.failedWith",
                            &[("message", &crate::i18n::app_error_text(&e))],
                        )),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    // —— 渲染 ——

    /// 视图 tab（Tauri `TopBarTabs` portal 同位：壳在顶栏渲染它，
    /// 页面主体的那行只剩频道胶囊——见 [`Self::render_gender_row`]）。
    pub(crate) fn render_top_tabs(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        let view = self.view;
        Some(
            h_flex()
                .id("top-new-tabs")
                .items_center()
                .gap_0p5()
                .children(
                    [
                        (
                            ViewTab::Recommend,
                            crate::i18n::t("newDrama.tabs.recommend"),
                        ),
                        (ViewTab::Calendar, crate::i18n::t("newDrama.tabs.calendar")),
                    ]
                    .map(|(tab, label)| {
                        let active = view == tab;
                        let button = crate::pill_tab::pill_tab(
                            format!("new-tab-{label}"),
                            label,
                            active,
                            cx,
                        );
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            if this.view == tab {
                                return;
                            }
                            this.view = tab;
                            if tab == ViewTab::Calendar && !this.cal_loaded {
                                this.load_calendar(None, false, cx);
                            }
                            cx.notify();
                        }))
                    }),
                )
                .into_any_element(),
        )
    }

    /// 频道胶囊行（Tauri 新剧页同款：只属于新剧推荐视图、贴左排）。
    ///
    /// Tauri 代码里写的是 `justify-end`，但 ToggleGroup 基础类是 inline-flex
    /// （宽度收缩到内容），`justify-end` 实际不生效——真机渲染是贴左。这里
    /// 以真机截图为口径（2026-10-17 并排对拍），不照抄失效的类名。
    fn render_gender_row(&self, cx: &mut Context<Self>) -> Option<gpui_kit::AnyElement> {
        if self.view != ViewTab::Recommend {
            return None;
        }
        let theme = cx.theme();
        let gender = self.gender;
        Some(
            h_flex()
                .id("new-gender-row")
                .w_full()
                .items_center()
                .gap_2()
                .children(
                    [
                        (2, crate::i18n::t("newDrama.gender.all")),
                        (1, crate::i18n::t("newDrama.gender.male")),
                        (0, crate::i18n::t("newDrama.gender.female")),
                    ]
                    .map(|(value, label)| {
                        let active = gender == value;
                        let button = crate::pill_tab::pill_tab(
                            format!("new-gender-{label}"),
                            label,
                            active,
                            cx,
                        );
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            this.switch_gender(value, cx);
                        }))
                    }),
                )
                .text_color(theme.foreground)
                .into_any_element(),
        )
    }

    fn render_recommend_card(
        &self,
        item: &RankItem,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let downloaded = self.downloaded.get(&item.series_id).copied().unwrap_or(0);
        let cover = self.covers.get(&item.cover).cloned();
        let mut tags: Vec<String> = item.tags.clone();
        if let Some(first) = item.sub_title.split('·').next()
            && !first.is_empty()
        {
            tags.push(first.to_string());
        }
        let tags: Vec<String> = tags.into_iter().filter(|t| !t.is_empty()).take(4).collect();
        v_flex()
            .id(ElementId::from(format!("new-card-{}", item.series_id)))
            .w_full()
            .overflow_hidden()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .cursor_pointer()
            .hover(|s| s.border_color(theme.foreground.opacity(0.3)).shadow_md())
            .on_click({
                let item = item.clone();
                cx.listener(move |this, _, _, cx| {
                    this.select_rank_item(&item, cx);
                })
            })
            .child(
                div()
                    .relative()
                    .w_full()
                    .aspect_ratio(3.0 / 4.0)
                    .flex_shrink_0()
                    .overflow_hidden()
                    .bg(theme.secondary)
                    .child(match cover {
                        Some(bitmap) => img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => div().size_full().into_any_element(),
                    })
                    .children((!item.rec_text.is_empty()).then(|| {
                        h_flex()
                            .absolute()
                            .bottom_1()
                            .left_1()
                            .items_center()
                            .gap_0p5()
                            .text_size(gpui_kit::px(10.0))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            // 封面角标热度走 star（Tauri 卡片 text-star 同款）
                            .text_color(crate::tokens::star(theme.mode))
                            .child(
                                gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Flame)
                                    .small(),
                            )
                            .child(SharedString::from(item.rec_text.clone()))
                    }))
                    .children((item.episode_cnt > 0).then(|| {
                        div()
                            .absolute()
                            .right_1()
                            .bottom_1()
                            .rounded(gpui_kit::px(4.0))
                            .bg(gpui_kit::rgba(0x00000099))
                            .px_1()
                            .py_0p5()
                            .text_size(gpui_kit::px(10.0))
                            .text_color(gpui_kit::rgb(0xffffffff))
                            .child(SharedString::from(crate::i18n::tf(
                                "newDrama.episodeTotal",
                                &[("count", &item.episode_cnt.to_string())],
                            )))
                    }))
                    .children((downloaded > 0).then(|| {
                        div().absolute().top_2().right_2().child(status_badge(
                            downloaded.to_string(),
                            theme.success.opacity(0.15),
                            theme.success,
                            theme.radius,
                        ))
                    })),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .p_2()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(SharedString::from(item.title.clone())),
                    )
                    .children((!tags.is_empty()).then(|| {
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(tags.join("·")))
                    }))
                    .children((item.score > 0.0).then(|| {
                        h_flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(
                                gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Star)
                                    .size_3p5()
                                    .text_color(crate::tokens::star(theme.mode)),
                            )
                            .child(SharedString::from(format!(
                                "{:.1}{}",
                                item.score,
                                crate::i18n::t("newDrama.scoreSuffix")
                            )))
                    })),
            )
            .into_any_element()
    }

    fn render_calendar_row(
        &self,
        item: &CalendarItem,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = self.covers.get(&item.cover).cloned();
        let reserved = item.has_subscribed || self.reserved_local.contains(&item.series_id);
        let card_item = item.clone();
        h_flex()
            .id(ElementId::from(format!("new-cal-{}", item.series_id)))
            .items_center()
            .gap_4()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .p_3()
            .cursor_pointer()
            .hover(|s| s.border_color(theme.foreground.opacity(0.3)).shadow_md())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_detail(
                    card_item.series_id.clone(),
                    calendar_prefill(&card_item, reserved),
                    cx,
                );
            }))
            .child(
                div()
                    // Tauri SeriesRowCard：封面 w-20（80）× 3:4（107）
                    .w(gpui_kit::px(80.0))
                    .h(gpui_kit::px(107.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded(theme.radius + gpui_kit::px(2.0))
                    .bg(theme.secondary)
                    .child(match cover {
                        Some(bitmap) => img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => div().size_full().into_any_element(),
                    }),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .child(SharedString::from(item.title.clone())),
                            )
                            .child(if item.is_online {
                                status_badge(
                                    crate::i18n::t("newDrama.online"),
                                    theme.success.opacity(0.15),
                                    theme.success,
                                    theme.radius,
                                )
                            } else {
                                status_badge(
                                    crate::i18n::t("newDrama.upcoming"),
                                    theme.secondary,
                                    theme.secondary_foreground,
                                    theme.radius,
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .children(item.rec_tags.first().map(|heat| {
                                div()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    // 预约人气走 hot（Tauri 日历行 text-hot 同款）
                                    .text_color(crate::tokens::hot(theme.mode))
                                    .child(SharedString::from(heat.clone()))
                            }))
                            .children((!item.category.is_empty()).then(|| {
                                div()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(item.category.clone()))
                            }))
                            .children((item.score > 0.0).then(|| {
                                // Tauri calendar-row：星标 + 数字（不留「分」）
                                h_flex()
                                    .items_center()
                                    .gap(gpui_kit::px(2.0))
                                    .child(
                                        gpui_kit::component::Icon::new(
                                            gpui_kit::assets::IconName::Star,
                                        )
                                        .size_3()
                                        .text_color(crate::tokens::star(theme.mode)),
                                    )
                                    .child(
                                        div().text_color(theme.muted_foreground).child(
                                            SharedString::from(format!("{:.1}", item.score)),
                                        ),
                                    )
                            }))
                            .children((item.episode_cnt > 0).then(|| {
                                // Tauri common.episodeCount =「{count} 集」（带空格）
                                div()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(crate::i18n::tf(
                                        "common.episodeCount",
                                        &[("count", &item.episode_cnt.to_string())],
                                    )))
                            })),
                    )
                    .children((!item.description.is_empty()).then(|| {
                        div()
                            .text_xs()
                            .line_height(gpui_kit::relative(1.625))
                            .text_color(theme.muted_foreground.opacity(0.8))
                            .line_clamp(2)
                            .overflow_hidden()
                            .child(SharedString::from(item.description.clone()))
                    })),
            )
            .child(
                v_flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1p5()
                    .children((item.publish_time > 0).then(|| {
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(format_publish_time(item.publish_time)))
                    }))
                    .children(if item.is_online {
                        let sid = item.series_id.clone();
                        let item_clone = item.clone();
                        vec![
                            Button::new(ElementId::from(format!(
                                "new-cal-play-{}",
                                item.series_id
                            )))
                            .icon(gpui_kit::assets::IconName::Play)
                            .label(crate::i18n::t("player.play"))
                            .outline()
                            .small()
                            .h_8()
                            .px_2p5()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.play_series(sid.clone(), cx);
                            }))
                            .into_any_element(),
                            Button::new(ElementId::from(format!(
                                "new-cal-detail-{}",
                                item.series_id
                            )))
                            .label(crate::i18n::t("nav.detail.title"))
                            .text()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.open_detail(
                                    item_clone.series_id.clone(),
                                    calendar_prefill(&item_clone, reserved),
                                    cx,
                                );
                            }))
                            .into_any_element(),
                        ]
                    } else {
                        let sid = item.series_id.clone();
                        vec![{
                            let button = Button::new(ElementId::from(format!(
                                "new-cal-reserve-{}",
                                item.series_id
                            )))
                            .icon(gpui_kit::assets::IconName::BellRing)
                            .label(if reserved {
                                crate::i18n::t("reservation.reserved")
                            } else {
                                crate::i18n::t("reservation.action")
                            })
                            .small()
                            .h_8()
                            .px_2p5()
                            .rounded(gpui_kit::px(999.0))
                            .disabled(reserved);
                            let button = if reserved {
                                button.secondary()
                            } else {
                                button.primary()
                            };
                            button
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.reserve(sid.clone(), cx);
                                }))
                                .into_any_element()
                        }]
                    }),
            )
            .into_any_element()
    }

    /// 预览种子（screenshot feature）：灌假推荐/假日历与合成封面。
    #[cfg(feature = "screenshot")]
    pub(crate) fn debug_preview_seed(&mut self, scene: &str, cx: &mut Context<Self>) {
        let calendar = scene == "new-calendar";
        self.view = if calendar {
            ViewTab::Calendar
        } else {
            ViewTab::Recommend
        };
        self.items = crate::preview::fixtures::new_drama_items();
        self.downloaded = HashMap::from([("7100001".to_string(), 3u32)]);
        for (i, item) in self.items.iter().enumerate() {
            self.covers
                .insert(item.cover.clone(), crate::preview::fixtures::cover(i));
        }
        if calendar {
            let items = crate::preview::fixtures::calendar_items(3);
            self.cal_dates = vec![
                "20261012".into(),
                "20261013".into(),
                "20261014".into(),
                "20261015".into(),
                "20261016".into(),
            ];
            self.cal_selected = "20261014".into();
            for (i, item) in items.iter().enumerate() {
                self.covers
                    .insert(item.cover.clone(), crate::preview::fixtures::cover(8 + i));
            }
            self.cal_items = items;
            self.cal_loaded = true;
            self.reserved_local = HashSet::from(["7100003".to_string()]);
        }
        cx.notify();
    }
}

impl gpui_kit::EventEmitter<NewDramaEvent> for NewDramaView {}

impl gpui_kit::Render for NewDramaView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 首拉加载（推荐/日历）：**整页**居中，性别筛选行等页内控件加载期
        // 不渲染——加载位置不受页内控件影响（用户定稿口径）。与创作者
        // 主页/排行榜的页面级分支同款。
        let page_loading = match self.view {
            ViewTab::Recommend => self.loading && self.items.is_empty(),
            ViewTab::Calendar => self.cal_loading && self.cal_items.is_empty(),
        };
        if page_loading {
            let theme = cx.theme();
            return v_flex()
                .size_full()
                .bg(theme.background)
                .text_color(theme.foreground)
                .items_center()
                .justify_center()
                .child(Spinner::new().small())
                .into_any_element();
        }
        let viewport_w = window.viewport_size().width.as_f32();
        let grid_cols = if viewport_w >= 1536.0 {
            8
        } else if viewport_w >= 1024.0 {
            5
        } else if viewport_w >= 768.0 {
            4
        } else if viewport_w >= 640.0 {
            3
        } else {
            2
        };
        let gender_row = self.render_gender_row(cx);
        let theme = cx.theme();
        // 颜色先取值（Hsla: Copy）：后面要构造吃 &mut cx 的闭包，不能再借 theme
        let muted_fg = theme.muted_foreground;
        let root_bg = theme.background;
        let root_fg = theme.foreground;
        let body: gpui_kit::AnyElement = if self.view == ViewTab::Recommend {
            if self.loading && self.items.is_empty() {
                h_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .py_8()
                    .child(Spinner::new().small())
                    .into_any_element()
            } else if self.failed && self.items.is_empty() {
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted_fg)
                            .child(crate::i18n::t("newDrama.loadFailed")),
                    )
                    .child(
                        Button::new("new-retry")
                            .label(crate::i18n::t("common.retry"))
                            .on_click(cx.listener(|this, _, _, cx| this.load_recommend(false, cx))),
                    )
                    .into_any_element()
            } else if self.items.is_empty() {
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted_fg)
                            .child(crate::i18n::t("newDrama.emptyRecommend")),
                    )
                    .into_any_element()
            } else {
                let items = self.items.clone();
                let has_more = self.has_more;
                let loading = self.loading;
                div()
                    .id("new-recommend")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .gap_4()
                            .child(
                                div().w_full().grid().grid_cols(grid_cols).gap_4().children(
                                    items
                                        .iter()
                                        .map(|item| self.render_recommend_card(item, cx)),
                                ),
                            )
                            .children((has_more).then(|| {
                                h_flex().w_full().justify_center().child(
                                    Button::new("new-more")
                                        .label(if loading {
                                            crate::i18n::t("feed.loadingMore")
                                        } else {
                                            crate::i18n::t("follow.loadMore")
                                        })
                                        .ghost()
                                        .small()
                                        .disabled(loading)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.load_recommend(true, cx);
                                        })),
                                )
                            }))
                            .children((!has_more).then(|| {
                                h_flex().w_full().justify_center().py_2().child(
                                    div()
                                        .text_sm()
                                        .text_color(muted_fg)
                                        .child(crate::i18n::t("follow.noMore")),
                                )
                            })),
                    )
                    .into_any_element()
            }
        } else {
            // 日历视图
            if self.cal_loading && self.cal_items.is_empty() {
                h_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .py_8()
                    .child(Spinner::new().small())
                    .into_any_element()
            } else if self.cal_failed && self.cal_items.is_empty() {
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted_fg)
                            .child(crate::i18n::t("newDrama.loadFailed")),
                    )
                    .child(
                        Button::new("new-cal-retry")
                            .label(crate::i18n::t("common.retry"))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.load_calendar(None, false, cx)),
                            ),
                    )
                    .into_any_element()
            } else {
                let selected = self.cal_selected.clone();
                let dates = self.cal_dates.clone();
                let rows: Vec<CalendarItem> = self
                    .cal_items
                    .iter()
                    .filter(|i| selected.is_empty() || selected_date_of(i) == selected)
                    .cloned()
                    .collect();
                let has_more = self.cal_has_more;
                let loading = self.cal_loading;
                let empty = rows.is_empty();
                div()
                    .id("new-calendar")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                h_flex()
                                    .w_full()
                                    .flex_wrap()
                                    .items_center()
                                    .gap_2()
                                    .pb_1()
                                    .children(dates.iter().map(|date| {
                                        let date = date.clone();
                                        let active = date == selected;
                                        // Tauri 日期钮：h-14 min-w-14 flex-1 max-w-20
                                        // 的描边卡片，选中 primary 实底（原来只是
                                        // 文字/小胶囊，卡片形态缺失）
                                        div()
                                            .id(ElementId::from(format!("new-date-{date}")))
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .justify_center()
                                            .h(gpui_kit::px(56.0))
                                            .min_w(gpui_kit::px(56.0))
                                            .max_w(gpui_kit::px(80.0))
                                            .flex_1()
                                            .rounded(theme.radius + gpui_kit::px(2.0))
                                            .border_1()
                                            .px(gpui_kit::px(4.0))
                                            .py(gpui_kit::px(6.0))
                                            .cursor_pointer()
                                            .when(active, |s| {
                                                s.bg(theme.primary)
                                                    .border_color(theme.primary)
                                                    .text_color(theme.primary_foreground)
                                            })
                                            .when(!active, |s| {
                                                s.bg(theme.background)
                                                    .border_color(theme.border)
                                                    .text_color(theme.foreground)
                                                    .hover(|h| h.bg(theme.accent))
                                            })
                                            .child(
                                                div()
                                                    .text_size(gpui_kit::px(11.0))
                                                    .line_height(gpui_kit::relative(1.2))
                                                    .child(SharedString::from(weekday_label(
                                                        &date,
                                                    ))),
                                            )
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                                    .line_height(gpui_kit::relative(1.2))
                                                    .child(SharedString::from(day_month(&date))),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if this.cal_selected == date {
                                                    return;
                                                }
                                                this.cal_selected = date.clone();
                                                this.cal_items.clear();
                                                this.cal_next_offset = 0;
                                                this.cal_session_id.clear();
                                                this.cal_has_more = false;
                                                this.load_calendar(Some(date.clone()), false, cx);
                                                cx.notify();
                                            }))
                                    })),
                            )
                            .children(if empty && has_more {
                                vec![
                                    h_flex()
                                        .w_full()
                                        .justify_center()
                                        .py_8()
                                        .child(Spinner::new().small())
                                        .into_any_element(),
                                ]
                            } else if empty {
                                vec![
                                    v_flex()
                                        .w_full()
                                        .items_center()
                                        .py_16()
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(muted_fg)
                                                .child(crate::i18n::t("newDrama.empty")),
                                        )
                                        .into_any_element(),
                                ]
                            } else {
                                rows.iter()
                                    .map(|item| self.render_calendar_row(item, cx))
                                    .collect()
                            })
                            .children((has_more && !empty).then(|| {
                                h_flex().w_full().justify_center().child(
                                    Button::new("new-cal-more")
                                        .label(if loading {
                                            crate::i18n::t("feed.loadingMore")
                                        } else {
                                            crate::i18n::t("follow.loadMore")
                                        })
                                        .ghost()
                                        .small()
                                        .disabled(loading)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.load_calendar(None, true, cx);
                                        })),
                                )
                            }))
                            .children((!has_more && !empty).then(|| {
                                h_flex().w_full().justify_center().py_2().child(
                                    div()
                                        .text_sm()
                                        .text_color(muted_fg)
                                        .child(crate::i18n::t("follow.noMore")),
                                )
                            })),
                    )
                    .into_any_element()
            }
        };
        v_flex()
            .size_full()
            .bg(root_bg)
            .text_color(root_fg)
            .id("new-page")
            .child(
                v_flex()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .px_4()
                    .pt_3()
                    .gap_3()
                    .children(gender_row)
                    .child(body),
            )
            .into_any_element()
    }
}

/// 日历条目的日期归属（"20261014" 形态；解析 `publish_time` 本地日期）。
fn selected_date_of(item: &CalendarItem) -> String {
    use chrono::{Datelike, TimeZone};
    match chrono::Local
        .timestamp_millis_opt(item.publish_time)
        .single()
    {
        Some(dt) => format!("{:04}{:02}{:02}", dt.year(), dt.month(), dt.day()),
        None => String::new(),
    }
}

/// "20261014" → 今天/周X。
fn weekday_label(date: &str) -> String {
    if date.len() != 8 {
        return String::new();
    }
    use chrono::{Datelike, Local};
    let today = Local::now();
    let today_key = format!("{:04}{:02}{:02}", today.year(), today.month(), today.day());
    if date == today_key {
        return crate::i18n::t("newDrama.today");
    }
    let Ok(d) = chrono::NaiveDate::parse_from_str(date, "%Y%m%d") else {
        return String::new();
    };
    let keys = [
        "newDrama.weekday.0",
        "newDrama.weekday.1",
        "newDrama.weekday.2",
        "newDrama.weekday.3",
        "newDrama.weekday.4",
        "newDrama.weekday.5",
        "newDrama.weekday.6",
    ];
    keys.get(d.weekday().num_days_from_sunday() as usize)
        .map(|key| crate::i18n::t(key))
        .unwrap_or_default()
}

/// "20261014" → "10/14"。
fn day_month(date: &str) -> String {
    if date.len() != 8 {
        return date.to_string();
    }
    format!(
        "{}/{}",
        date[4..6].trim_start_matches('0'),
        date[6..8].trim_start_matches('0')
    )
}

/// UTC 毫秒 → "09-27 12:00"（Tauri formatPublishTime 同款）。
fn format_publish_time(ms: i64) -> String {
    use chrono::{Datelike, TimeZone, Timelike};
    match chrono::Local.timestamp_millis_opt(ms).single() {
        Some(dt) => format!(
            "{:02}-{:02} {:02}:{:02}",
            dt.month(),
            dt.day(),
            dt.hour(),
            dt.minute()
        ),
        None => String::new(),
    }
}

fn status_badge(
    label: impl Into<SharedString>,
    background: gpui_kit::Hsla,
    foreground: gpui_kit::Hsla,
    radius: gpui_kit::Pixels,
) -> gpui_kit::AnyElement {
    h_flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .rounded(radius)
        .bg(background)
        .border_1()
        .border_color(background)
        .text_color(foreground)
        .px_2()
        .py_0p5()
        .text_xs()
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .whitespace_nowrap()
        .child(label.into())
        .into_any_element()
}

/// 日历条目 → 详情 prefill（rec_tags 当题材标签，与 Tauri 同口径）。
fn calendar_prefill(item: &CalendarItem, reserved: bool) -> DetailPrefill {
    DetailPrefill {
        title: item.title.clone(),
        cover: item.cover.clone(),
        tags: item
            .rec_tags
            .iter()
            .filter(|t| !t.is_empty())
            .cloned()
            .collect(),
        desc: item.description.clone(),
        reserved,
    }
}

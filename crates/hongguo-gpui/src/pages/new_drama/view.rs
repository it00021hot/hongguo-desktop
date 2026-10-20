use super::*;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Disableable, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, Window, div};

impl gpui_kit::Render for NewDramaView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 封面预热：排上转码（gpui 资产机制去重），成品图落盘后 gpui 刷新窗口。
        let mut cover_urls: Vec<String> = self.items.iter().map(|i| i.cover.clone()).collect();
        cover_urls.extend(self.cal_items.iter().map(|i| i.cover.clone()));
        crate::cover_img::prewarm(&cover_urls, hongguo_protocol::cover::EDGE_CARD, cx);
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
        // 滚动哨兵：滚到底自动续页（Tauri new-calendar-view.tsx /
        // new-drama-recommends.tsx 的 IntersectionObserver 同语义；两个视图
        // 各有自己的滚动容器，按当前 tab 判）。
        match self.view {
            ViewTab::Recommend => {
                if self
                    .scroll_recommend
                    .should_load(self.has_more, self.loading)
                {
                    self.load_recommend(true, cx);
                }
            }
            ViewTab::Calendar => {
                if self
                    .scroll_calendar
                    .should_load(self.cal_has_more, self.cal_loading)
                {
                    self.load_calendar(None, true, cx);
                }
            }
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
                    .track_scroll(self.scroll_recommend.handle())
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
                    .track_scroll(self.scroll_calendar.handle())
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

impl NewDramaView {
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
            crate::preview::fixtures::put_preview_image(
                item.cover.clone(),
                crate::preview::fixtures::cover(i),
            );
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
                crate::preview::fixtures::put_preview_image(
                    item.cover.clone(),
                    crate::preview::fixtures::cover(8 + i),
                );
            }
            self.cal_items = items;
            self.cal_loaded = true;
            self.reserved_local = HashSet::from(["7100003".to_string()]);
        }
        cx.notify();
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

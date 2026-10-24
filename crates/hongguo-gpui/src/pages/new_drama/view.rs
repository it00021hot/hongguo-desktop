use super::*;

use gpui_kit::component::button::Button;
use gpui_kit::component::h_flex;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable};
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
        // 滚动哨兵（虚拟列表版）：滚到列表末尾就续页；新页未测高时
        // is_scrolled_to_end 报 None，天然防住「追加后仍在底部」的连环翻页。
        match self.view {
            ViewTab::Recommend => {
                if self.has_more && !self.loading && self.rec_rows.scrolled_to_end() {
                    self.load_recommend(true, cx);
                }
            }
            ViewTab::Calendar => {
                if self.cal_has_more && !self.cal_loading && self.cal_rows.scrolled_to_end() {
                    self.load_calendar(None, true, cx);
                }
            }
        }
        // 换频道/切日期撞上在途请求而欠下的重拉：在途已落定，这里补发
        // （switch_gender / 日期点击的登记，见 filter.rs 与本文件日期钮）。
        match self.view {
            ViewTab::Recommend if self.recommend_needs_reload && !self.loading => {
                self.recommend_needs_reload = false;
                self.load_recommend(false, cx);
            }
            ViewTab::Calendar if self.calendar_needs_reload && !self.cal_loading => {
                self.calendar_needs_reload = false;
                let date = self.cal_selected.clone();
                self.load_calendar((!date.is_empty()).then_some(date), false, cx);
            }
            _ => {}
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
                // 换频道后台重拉中（keepPreviousData 展示态）：Tauri
                // RefreshShade 同款——降透明度 + 挡掉鼠标
                let refreshing = self.loading && !self.items.is_empty();
                // 虚拟列表：按列数分块成行（行高 = 行内最高卡），末位固定
                // footer 槽（加载更多/到底提示）。可见行才参与布局/解码/绘制。
                self.rec_rows.sync(self.items.chunks(grid_cols).count() + 1);
                let entity = cx.entity();
                let state = self.rec_rows.state();
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .when(refreshing, |s| s.opacity(0.5))
                    // Auto sizing 的 list() 不自报内容高，必须从父容器拿确定高度
                    .child(
                        gpui_kit::list(state, move |ix, window, cx| {
                            entity.update(cx, |view, cx| {
                                view.render_rec_row(ix, grid_cols, window, cx)
                            })
                        })
                        .flex_1(),
                    )
                    // 刷新遮罩：挡掉旧列表上的鼠标交互
                    .children(refreshing.then(|| {
                        div()
                            .id("new-recommend-shade")
                            .absolute()
                            .inset_0()
                            .occlude()
                    }))
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
                let visible_ixs: Vec<usize> = self
                    .cal_items
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| selected.is_empty() || selected_date_of(i) == selected)
                    .map(|(ix, _)| ix)
                    .collect();
                let has_more = self.cal_has_more;
                let loading = self.cal_loading;
                let empty = visible_ixs.is_empty();
                // 行区：混排页过滤后暂空给转圈/空态；否则挂虚拟行列表
                //（末位固定 footer 槽：加载更多/到底提示），可见行才参与
                // 布局/解码/绘制。
                let rows_region: gpui_kit::AnyElement = if empty && (has_more || loading) {
                    h_flex()
                        .w_full()
                        .justify_center()
                        .py_8()
                        .child(Spinner::new().small())
                        .into_any_element()
                } else if empty {
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
                        .into_any_element()
                } else {
                    self.cal_rows.sync(visible_ixs.len() + 1);
                    let entity = cx.entity();
                    let state = self.cal_rows.state();
                    gpui_kit::list(state, move |ix, _window, cx| {
                        entity.update(cx, |view, cx| view.render_cal_row(ix, &visible_ixs, cx))
                    })
                    .flex_1()
                    .into_any_element()
                };
                // 日期条常驻（虚拟列表自己滚——原来日期条跟内容一起滚走）
                v_flex()
                    .w_full()
                    .flex_1()
                    .min_h_0()
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
                                            .child(SharedString::from(weekday_label(&date))),
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
                                        // 旧日期的行保留展示（Tauri
                                        // keepPreviousData 同语义）：按新日期
                                        // 过滤后为空时渲染层给转圈，新数据
                                        // 到达整替；翻页游标作废防串页，滚动
                                        // 归零（换 key 重挂载同语义）。
                                        this.cal_next_offset = 0;
                                        this.cal_session_id.clear();
                                        this.cal_has_more = false;
                                        // 虚拟列表按整替对账：reset 清测量并滚回顶
                                        this.cal_rows.replace();
                                        if this.cal_loading {
                                            // 在途请求未落定：登记待重拉，落定
                                            // 后由 render 补发（单飞闸会吞掉直接
                                            // 调用，漏了这步新日期永远拉不到）
                                            this.calendar_needs_reload = true;
                                        } else {
                                            this.load_calendar(Some(date.clone()), false, cx);
                                        }
                                        cx.notify();
                                    }))
                            })),
                    )
                    .child(
                        div()
                            .id("new-calendar")
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .child(rows_region),
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

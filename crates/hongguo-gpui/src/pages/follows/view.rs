//! 追更页渲染（Tauri `/follows` = `follows-page.tsx` 对齐的视图侧）。
//!
//! 数据与操作在 `mod.rs`；本文件只画——双 tab 胶囊 / 管理行 / 追更卡 /
//! 列表与提醒两个 body。拆出为独立文件：新增文件 800 行闸门的结构拆分，
//! 纯移动零行为变更。

use gpui_kit::component::Disableable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, SharedString, div, img};
use hongguo_api::interact::FollowUpdateItem;

use super::Tab;
use crate::pages::list_common::pick_dot;

impl super::FollowsView {
    // —— 渲染 ——

    /// 顶部 tab 胶囊（Tauri 圆角胶囊形态：选中主色填充，未选 muted）。
    pub(super) fn render_tabs(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let unread = self.unread_count();
        let pill = |active: bool| {
            div()
                .rounded_full()
                .px_4()
                .py_1p5()
                .text_sm()
                .cursor_pointer()
                .when(active, |s| {
                    s.bg(theme.primary)
                        .text_color(theme.primary_foreground)
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                })
                .when(!active, |s| {
                    s.bg(theme.muted)
                        .text_color(theme.muted_foreground)
                        .hover(|s| s.text_color(theme.foreground))
                })
        };
        h_flex()
            .w_full()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                pill(self.tab == Tab::Follows)
                    .id("follows-tab-list")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab = Tab::Follows;
                        cx.notify();
                    }))
                    .child("追更的剧"),
            )
            .child(
                pill(self.tab == Tab::Notices)
                    .id("follows-tab-notices")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab = Tab::Notices;
                        cx.notify();
                    }))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .child("更新提醒")
                            .children((unread > 0).then(|| {
                                div()
                                    .rounded_full()
                                    .bg(theme.danger)
                                    .px_1p5()
                                    .py_0p5()
                                    .text_size(gpui_kit::px(10.0))
                                    .text_color(theme.danger_foreground)
                                    .child(SharedString::from(unread.to_string()))
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 追更的剧：管理模式开关行（Tauri 独立一行、左对齐）。
    pub(super) fn render_manage_row(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        h_flex()
            .w_full()
            .items_center()
            .child(
                Button::new("follows-manage")
                    .label(if self.managing {
                        "退出管理"
                    } else {
                        "管理"
                    })
                    .outline()
                    .small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.managing = !this.managing;
                        if !this.managing {
                            this.selected.clear();
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// 追更卡：封面 + 标题 + 状态行 + 集数进度 + 动作列（播放 / 取消追更）。
    fn render_card(
        &self,
        item: &FollowUpdateItem,
        managing: bool,
        picked: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let cover = self.covers.get(&item.series_id).cloned();
        // 状态行（player p() 同款拼装）：追平/未看集数 + 连载中/已完结
        let remain = item.episode_cnt - item.latest_watched_vid_index;
        let progress = if item.latest_watched_vid_index > 0 {
            if remain <= 0 {
                "已追平".to_string()
            } else {
                format!("{remain} 集未看")
            }
        } else {
            String::new()
        };
        let serial = if item.finished {
            "已完结"
        } else {
            "连载中"
        };
        let status = [progress.as_str(), serial]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" · ");
        let cancelling = self.cancelling.contains(&item.series_id);
        h_flex()
            .id(ElementId::from(format!("follow-card-{}", item.series_id)))
            .items_stretch()
            .gap_4()
            .rounded(gpui_kit::px(12.0))
            .border_1()
            .border_color(if picked { theme.primary } else { theme.border })
            .bg(if picked {
                theme.secondary
            } else {
                theme.group_box
            })
            .p_3()
            .hover(|s| s.border_color(theme.foreground.opacity(0.3)))
            .children(managing.then(|| {
                let sid = item.series_id.clone();
                div().flex().items_center().child(pick_dot(
                    ElementId::from(format!("follow-pick-{}", item.series_id)),
                    picked,
                    cx.listener(move |this, _, _, cx| this.toggle_selected(&sid, cx)),
                    cx,
                ))
            }))
            // 封面（点击：管理态选/取消，否则进详情）
            .child(
                div()
                    .id(ElementId::from(format!("follow-cover-{}", item.series_id)))
                    .relative()
                    .w(gpui_kit::px(92.0))
                    .h(gpui_kit::px(123.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded(gpui_kit::px(8.0))
                    .bg(theme.secondary)
                    .cursor_pointer()
                    .on_click({
                        let item = item.clone();
                        cx.listener(move |this, _, _, cx| {
                            if this.managing {
                                this.toggle_selected(&item.series_id, cx);
                            } else {
                                this.open_detail(&item, cx);
                            }
                        })
                    })
                    .child(match cover {
                        Some(bitmap) => img(ImageSource::Render(bitmap))
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => h_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .child(
                                gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Tv)
                                    .text_color(theme.muted_foreground),
                            )
                            .into_any_element(),
                    }),
            )
            // 标题 / 状态行 / 集数进度
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .justify_center()
                    .gap_1()
                    .child(
                        div()
                            .id(ElementId::from(format!("follow-title-{}", item.series_id)))
                            .truncate()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .cursor_pointer()
                            .on_click({
                                let item = item.clone();
                                cx.listener(move |this, _, _, cx| {
                                    if this.managing {
                                        this.toggle_selected(&item.series_id, cx);
                                    } else {
                                        this.open_detail(&item, cx);
                                    }
                                })
                            })
                            .child(SharedString::from(item.title.clone())),
                    )
                    .children((!status.is_empty()).then(|| {
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(status.clone()))
                    }))
                    .children((item.episode_cnt > 0).then(|| {
                        div().text_xs().text_color(theme.muted_foreground).child(
                            SharedString::from(format!(
                                "看到 {} / {} 集",
                                item.latest_watched_vid_index, item.episode_cnt
                            )),
                        )
                    })),
            )
            // 动作列（管理态不渲染）
            .children((!managing).then(|| {
                let sid = item.series_id.clone();
                let finished = item.finished;
                v_flex()
                    .flex_shrink_0()
                    .justify_center()
                    .gap_1p5()
                    .child(
                        Button::new(ElementId::from(format!("follow-play-{}", item.series_id)))
                            .icon(gpui_kit::assets::IconName::Play)
                            .label("播放")
                            .outline()
                            .small()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.play_series(&sid, cx);
                            })),
                    )
                    // 已完结剧不再追更（无新集可等；Tauri 同款隐藏）
                    .children((!finished).then(|| {
                        let sid = item.series_id.clone();
                        Button::new(ElementId::from(format!("follow-cancel-{}", item.series_id)))
                            .icon(gpui_kit::assets::IconName::Tv)
                            .label("取消追更")
                            .outline()
                            .small()
                            .border_color(theme.primary)
                            .text_color(theme.primary)
                            .disabled(cancelling)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.cancel_follow(sid.clone(), cx);
                            }))
                    }))
            }))
            .into_any_element()
    }

    pub(super) fn render_follows_body(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if self.loading && self.items.is_empty() {
            return h_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(Spinner::new().small())
                .into_any_element();
        }
        if let Some(err) = self.failed.clone() {
            let theme = cx.theme();
            return v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("追更列表加载失败"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(SharedString::from(err)),
                )
                .child(
                    Button::new("follows-retry")
                        .label("重试")
                        .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                )
                .into_any_element();
        }
        if self.items.is_empty() {
            let theme = cx.theme();
            return v_flex()
                .items_center()
                // Tauri Empty：流内 py-16 顶部对齐
                .py_16()
                .gap_2()
                .child(
                    gpui_kit::component::Icon::new(gpui_kit::assets::IconName::Tv)
                        .with_size(gpui_kit::px(32.0))
                        .text_color(theme.muted_foreground),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("还没有追更的剧"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("在剧集详情页点「追更」，这部剧更新时会提醒你"),
                )
                .into_any_element();
        }
        let managing = self.managing;
        let mut column = v_flex().w_full().gap_2();
        for item in self.items.clone() {
            let picked = self.selected.contains(&item.series_id);
            column = column.child(self.render_card(&item, managing, picked, cx));
        }
        if self.has_more {
            column = column.child(
                h_flex().w_full().justify_center().pt_2().child(
                    Button::new("follows-more")
                        .label(if self.loading_more {
                            "加载中…"
                        } else {
                            "加载更多"
                        })
                        .outline()
                        .small()
                        .disabled(self.loading_more)
                        .on_click(cx.listener(|this, _, _, cx| this.load_more(cx))),
                ),
            );
        } else {
            let muted = cx.theme().muted_foreground;
            column = column.child(
                h_flex()
                    .w_full()
                    .justify_center()
                    .pt_2()
                    .child(div().text_xs().text_color(muted).child("没有更多了")),
            );
        }
        div()
            .id("follows-list")
            .flex_1()
            .overflow_y_scroll()
            .p_4()
            .child(column)
            .into_any_element()
    }

    pub(super) fn render_notices_body(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let mut column = v_flex().w_full().gap_2();
        for notice in &self.notices {
            let unread = !notice.read && !self.read_all;
            let kind_online = notice.kind == hongguo_common::model::NOTICE_KIND_ONLINE;
            column = column.child(
                div()
                    .id(ElementId::from(format!("follow-notice-{}", notice.id)))
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded(gpui_kit::px(12.0))
                    .border_1()
                    .border_color(if unread { theme.primary } else { theme.border })
                    .bg(theme.group_box)
                    .p_3()
                    .cursor_pointer()
                    .hover(|s| s.border_color(theme.foreground.opacity(0.3)))
                    .on_click({
                        let notice = notice.clone();
                        cx.listener(move |this, _, _, cx| {
                            this.open_notice(&notice, cx);
                        })
                    })
                    .child(
                        div()
                            .flex_shrink_0()
                            .rounded(gpui_kit::px(4.0))
                            .bg(if kind_online {
                                theme.primary.opacity(0.15)
                            } else {
                                theme.muted
                            })
                            .px_1p5()
                            .py_0p5()
                            .text_size(gpui_kit::px(10.0))
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .text_color(if kind_online {
                                theme.primary
                            } else {
                                theme.muted_foreground
                            })
                            .child(if kind_online {
                                "预约上线"
                            } else {
                                "追更更新"
                            }),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .gap_0p5()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .child(SharedString::from(notice.title.clone())),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(notice.detail.clone())),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(SharedString::from(format_notice_time(notice.at))),
                    )
                    .children(unread.then(|| {
                        div()
                            .size_2()
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(theme.danger)
                    })),
            );
        }
        let notices_empty = self.notices.is_empty();
        let controls = h_flex()
            .w_full()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                Checkbox::new("follows-popup-on")
                    .checked(self.popup_on)
                    .label("弹出提醒")
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.set_popup_on(*checked, cx);
                    })),
            )
            .child(
                Button::new("follows-check-updates")
                    .icon(gpui_kit::assets::IconName::RefreshCw)
                    .label(if self.checking {
                        "检查中…"
                    } else {
                        "检查更新"
                    })
                    .outline()
                    .small()
                    .disabled(self.checking)
                    .on_click(cx.listener(|this, _, _, cx| this.check_updates(cx))),
            )
            .children((!notices_empty).then(|| {
                Button::new("follows-clear")
                    .icon(gpui_kit::assets::IconName::CheckCheck)
                    .label("清空")
                    .ghost()
                    .small()
                    .disabled(self.clearing)
                    .on_click(cx.listener(|this, _, _, cx| this.clear_notices(cx)))
            }));
        let body = if notices_empty {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    gpui_kit::component::Icon::new(gpui_kit::assets::IconName::BellRing)
                        .with_size(gpui_kit::px(32.0))
                        .text_color(theme.muted_foreground),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("暂无提醒"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("追的剧更新、预约的剧上线后，会在这里提醒你（每 20 分钟自动检查）"),
                )
                .into_any_element()
        } else {
            div()
                .id("follows-notices-list")
                .flex_1()
                .overflow_y_scroll()
                .p_4()
                .child(column)
                .into_any_element()
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .gap_3()
            .child(div().w_full().px_4().pt_3().child(controls))
            .child(body)
            .into_any_element()
    }
}

/// unix 秒 → 「10月15日 12:24」（本地时区；Tauri Intl 格式化的同语义简化）。
fn format_notice_time(secs: i64) -> String {
    use chrono::{Datelike, TimeZone, Timelike};
    match chrono::Local.timestamp_opt(secs, 0).single() {
        Some(dt) => format!(
            "{}月{}日 {:02}:{:02}",
            dt.month(),
            dt.day(),
            dt.hour(),
            dt.minute()
        ),
        None => String::new(),
    }
}

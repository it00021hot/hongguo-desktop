//! 追更页渲染（Tauri `/follows` = `follows-page.tsx` 对齐的视图侧）。
//!
//! 数据与操作在 `mod.rs`；本文件只画——双 tab 胶囊 / 管理行 / 追更卡 /
//! 列表与提醒两个 body。拆出为独立文件：新增文件 800 行闸门的结构拆分，
//! 纯移动零行为变更。

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::Disableable;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, Role, SharedString, Toggled, div};
use hongguo_api::interact::FollowUpdateItem;

use super::Tab;
use crate::pages::list_common::pick_dot;
use crate::ui_button::button as Button;

impl super::FollowsView {
    // —— 渲染 ——

    /// 顶部 tab 胶囊（Tauri 圆角胶囊形态：选中主色填充，未选 muted）。
    ///
    /// 载体是 gpui-kit 的 base Button 而不是 div：div 不带焦点句柄，键盘用户
    /// 根本够不到这两个 tab；base Button 自带焦点句柄与 Enter/Space 激活。
    pub(super) fn render_tabs(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let unread = self.unread_count();
        let pill = |id: ElementId, label: SharedString, active: bool| {
            BaseButton::new(id)
                .role(Role::RadioButton)
                .aria_toggled(if active {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .accessibility_label(label.clone())
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
                // base Button 固定 justify_center，这里改回左对齐，和原来的 div 一致。
                .justify_start()
                // base Button 强制 line_height(relative(1.))，文字行盒会比默认矮一截、
                // 胶囊整体变矮；钉回窗口默认行高（黄金比例）才与原来的 div 像素一致。
                .line_height(gpui_kit::phi())
                // 键盘焦点没有默认焦点环：按 Tauri/shadcn 同款补 border-ring + 3px 外圈。
                .focus_visible(|s| {
                    s.border_color(theme.ring)
                        .shadow(crate::tokens::focus_ring(theme))
                })
        };
        h_flex()
            .id("follows-tabs")
            .role(Role::RadioGroup)
            .w_full()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                pill(
                    ElementId::from("follows-tab-list"),
                    SharedString::from(crate::i18n::t("follow.tabFollows")),
                    self.tab == Tab::Follows,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.tab = Tab::Follows;
                    cx.notify();
                }))
                .child(crate::i18n::t("follow.tabFollows")),
            )
            .child(
                pill(
                    ElementId::from("follows-tab-notices"),
                    SharedString::from(crate::i18n::t("follow.tabNotices")),
                    self.tab == Tab::Notices,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.tab = Tab::Notices;
                    cx.notify();
                }))
                .child(
                    h_flex()
                        .items_center()
                        .gap_1p5()
                        .child(crate::i18n::t("follow.tabNotices"))
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
                        crate::i18n::t("batch.exitManage")
                    } else {
                        crate::i18n::t("batch.manage")
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
        let cover = crate::cover_img::card(&item.cover, crate::cover_img::CoverShape::Inline);
        // 状态行（player p() 同款拼装）：追平/未看集数 + 连载中/已完结
        let remain = item.episode_cnt - item.latest_watched_vid_index;
        let progress = if item.latest_watched_vid_index > 0 {
            if remain <= 0 {
                crate::i18n::t("follow.caughtUp")
            } else {
                crate::i18n::tf("follow.unwatched", &[("count", &remain.to_string())])
            }
        } else {
            String::new()
        };
        let serial = if item.finished {
            crate::i18n::t("follow.finished")
        } else {
            crate::i18n::t("follow.serializing")
        };
        let status = [progress.as_str(), serial.as_str()]
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
            .rounded(crate::tokens::radius::CARD)
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
                    item.title.clone(),
                    cx.listener(move |this, _, _, cx| this.toggle_selected(&sid, cx)),
                    cx,
                ))
            }))
            // 封面（点击：管理态选/取消，否则进详情）
            .child(
                // 封面可点 → base::Button 承担 role/focus/Enter-Space（视觉仍由
                // styled 链决定）。库强制 flex row + 居中：这里复原原 div 的
                // 块级竖排与左上对齐，固定 92×123 的盒子不被 flex 改写。
                BaseButton::new(ElementId::from(format!("follow-cover-{}", item.series_id)))
                    .accessibility_label(item.title.clone())
                    .relative()
                    .w(gpui_kit::px(92.0))
                    .h(gpui_kit::px(123.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded(crate::tokens::radius::COVER)
                    .bg(theme.secondary)
                    .cursor_pointer()
                    .flex_col()
                    .items_stretch()
                    .justify_start()
                    // base Button 强制 line_height(relative(1.))，文字行盒比默认矮一
                    // 截；钉回默认行高才与原 div 像素一致。
                    .line_height(gpui_kit::phi())
                    // 键盘焦点没有默认焦点环：按 Tauri/shadcn 同款补 border-ring + 3px 外圈。
                    .focus_visible(|s| {
                        s.border_color(theme.ring)
                            .shadow(crate::tokens::focus_ring(theme))
                    })
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
                    .child(cover),
            )
            // 标题 / 状态行 / 集数进度
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .justify_center()
                    .gap_1()
                    .child(
                        // 标题可点 → base::Button（Tauri 侧 series-row-card 的
                        // `article role="button"` 同语义）。flex_col + items_stretch
                        // 复原原 div 的块级排版，标题仍按卡宽截断。
                        BaseButton::new(ElementId::from(format!(
                            "follow-title-{}",
                            item.series_id
                        )))
                        .accessibility_label(item.title.clone())
                        .truncate()
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .flex_col()
                        .items_stretch()
                        .justify_start()
                        // base Button 强制 line_height(relative(1.))，文字行盒比默认矮一
                        // 截；钉回默认行高才与原 div 像素一致。
                        .line_height(gpui_kit::phi())
                        // 键盘焦点没有默认焦点环：按 Tauri/shadcn 同款补 border-ring + 3px 外圈。
                        .focus_visible(|s| {
                            s.border_color(theme.ring)
                                .shadow(crate::tokens::focus_ring(theme))
                        })
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
                            SharedString::from(crate::i18n::tf(
                                "follow.episodeProgress",
                                &[
                                    ("watched", &item.latest_watched_vid_index.to_string()),
                                    ("total", &item.episode_cnt.to_string()),
                                ],
                            )),
                        )
                    })),
            )
            // 动作列（管理态不渲染）
            .children((!managing).then(|| {
                let sid = item.series_id.clone();
                let watched = item.latest_watched_vid_index;
                let finished = item.finished;
                v_flex()
                    .flex_shrink_0()
                    .justify_center()
                    .gap_1p5()
                    .child(
                        Button::new(ElementId::from(format!("follow-play-{}", item.series_id)))
                            .icon(gpui_kit::assets::IconName::Play)
                            .label(crate::i18n::t("player.play"))
                            .outline()
                            .small()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.play_series(&sid, Some(watched), cx);
                            })),
                    )
                    // 已完结剧不再追更（无新集可等；Tauri 同款隐藏）
                    .children((!finished).then(|| {
                        let sid = item.series_id.clone();
                        Button::new(ElementId::from(format!("follow-cancel-{}", item.series_id)))
                            .icon(gpui_kit::assets::IconName::Tv)
                            .label(crate::i18n::t("follow.unfollow"))
                            .outline()
                            .small()
                            .border_color(theme.primary)
                            .text_color(theme.primary)
                            .disabled(cancelling)
                            .when(cancelling, |b| b.cursor_default())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.cancel_follow(sid.clone(), cx);
                            }))
                    }))
            }))
            .into_any_element()
    }

    pub(super) fn render_follows_body(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        if self.loading && self.items.is_empty() {
            // 首拉加载：行骨架铺在列表容器里（与真实列表同 padding，
            // 数据到位不跳版）。行高按 GPUI 真实行算，不照抄 Tauri 的 h-28：
            // 封面 123 + 行内上下 p_3 24 + 行间距（列表 gap_2）8 = 155。
            return div()
                .id("follows-skeleton")
                .flex_1()
                .overflow_y_scroll()
                .p_4()
                .child(crate::pages::skeleton::rows(cx, 4, 147.0, 8.0))
                .into_any_element();
        }
        if let Some(err) = self.failed.clone() {
            return crate::pages::states::error_state(
                cx,
                None,
                crate::i18n::t("follow.loadFailed"),
                Some(err),
                Some((
                    "follows-retry",
                    crate::pages::states::retry_on(cx.entity(), |this, cx| this.load(cx)),
                )),
            );
        }
        if self.items.is_empty() {
            return crate::pages::states::empty_state(
                cx,
                Some(gpui_kit::assets::IconName::Tv),
                crate::i18n::t("follow.empty"),
                Some(crate::i18n::t("follow.emptyHint")),
            );
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
                            crate::i18n::t("common.loading")
                        } else {
                            crate::i18n::t("follow.loadMore")
                        })
                        .outline()
                        .small()
                        .disabled(self.loading_more)
                        .when(self.loading_more, |b| b.cursor_default())
                        .on_click(cx.listener(|this, _, _, cx| this.load_more(cx))),
                ),
            );
        } else {
            let muted = cx.theme().muted_foreground;
            column = column.child(
                h_flex().w_full().justify_center().pt_2().child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(crate::i18n::t("follow.noMore")),
                ),
            );
        }
        div()
            .id("follows-list")
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
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
                // 通知行可点 → base::Button 承担 role/focus/Enter-Space。原链已是
                // flex row + items_center，与库注入的一致，只需把 justify 改回
                // flex-start（库固定 justify_center）。
                BaseButton::new(ElementId::from(format!("follow-notice-{}", notice.id)))
                    .accessibility_label(notice.title.clone())
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded(crate::tokens::radius::CARD)
                    .border_1()
                    .border_color(if unread { theme.primary } else { theme.border })
                    .bg(theme.group_box)
                    .p_3()
                    .cursor_pointer()
                    .hover(|s| s.border_color(theme.foreground.opacity(0.3)))
                    .justify_start()
                    // base Button 强制 line_height(relative(1.))，文字行盒比默认矮一
                    // 截；钉回默认行高才与原 div 像素一致。
                    .line_height(gpui_kit::phi())
                    // 键盘焦点没有默认焦点环：按 Tauri/shadcn 同款补 border-ring + 3px 外圈。
                    .focus_visible(|s| {
                        s.border_color(theme.ring)
                            .shadow(crate::tokens::focus_ring(theme))
                    })
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
                                crate::i18n::t("follow.kindOnline")
                            } else {
                                crate::i18n::t("follow.kindFollow")
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
                    .label(crate::i18n::t("follow.popupOn"))
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.set_popup_on(*checked, cx);
                    })),
            )
            .child(
                Button::new("follows-check-updates")
                    .icon(gpui_kit::assets::IconName::RefreshCw)
                    .label(if self.checking {
                        crate::i18n::t("follow.checking")
                    } else {
                        crate::i18n::t("follow.checkUpdate")
                    })
                    .outline()
                    .small()
                    .disabled(self.checking)
                    .when(self.checking, |b| b.cursor_default())
                    .on_click(cx.listener(|this, _, _, cx| this.check_updates(cx))),
            )
            .children((!notices_empty).then(|| {
                Button::new("follows-clear")
                    .icon(gpui_kit::assets::IconName::CheckCheck)
                    .label(crate::i18n::t("follow.clearAll"))
                    .ghost()
                    .small()
                    .disabled(self.clearing)
                    .when(self.clearing, |b| b.cursor_default())
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
                        .child(crate::i18n::t("follow.noticesEmpty")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(crate::i18n::t("follow.noticesEmptyHint")),
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
        Some(dt) => match crate::i18n::locale() {
            crate::i18n::Locale::ZhCn => format!(
                "{}月{}日 {:02}:{:02}",
                dt.month(),
                dt.day(),
                dt.hour(),
                dt.minute()
            ),
            crate::i18n::Locale::EnUs => format!(
                "{:02}/{:02} {:02}:{:02}",
                dt.month(),
                dt.day(),
                dt.hour(),
                dt.minute()
            ),
        },
        None => String::new(),
    }
}

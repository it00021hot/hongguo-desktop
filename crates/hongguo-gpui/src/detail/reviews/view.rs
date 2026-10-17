//! 剧评 tab 渲染（Tauri ReviewList / ReviewComposer 对齐的视图侧）。
//!
//! 数据与操作在 `mod.rs`；本文件只画——发评框 / 剧评行（点赞/回复/删除）/
//! 回复线程 / 列表。拆出为独立文件：纯移动零行为变更（文件规模拆分）。

use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, ImageSource, ObjectFit, SharedString, div, img};
use hongguo_api::danmaku::ReplyItem;

use crate::detail::DetailView;
use crate::detail::header::{hot_color, star_color};

impl super::super::DetailView {
    pub(super) fn render_review_reply_row(
        &self,
        parent: &str,
        r: &ReplyItem,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let rid = r.reply_id.clone();
        let avatar = self.images.get(&r.avatar).cloned();
        let digged = self
            .review_digg_override
            .get(&rid)
            .copied()
            .unwrap_or(r.user_digg);
        let digg_count = r.digg_count;
        let mine = !self.my_user_id().is_empty() && r.user_id == self.my_user_id();
        let parent_id = parent.to_string();
        h_flex()
            .items_start()
            .gap_2()
            .child(
                div()
                    .size_6()
                    .flex_shrink_0()
                    .overflow_hidden()
                    .rounded_full()
                    .bg(theme.secondary)
                    .children(avatar.map(|bitmap| {
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
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        SharedString::from(if r.user_name.is_empty() {
                            crate::i18n::t("player.comments.anon")
                        } else {
                            r.user_name.clone()
                        }),
                    ))
                    .child(div().text_sm().child(SharedString::from(format!(
                        "{} @{}：{}",
                        crate::i18n::t("detail.replyToPrefix"),
                        if r.reply_to_name.is_empty() {
                            crate::i18n::t("player.comments.originalComment")
                        } else {
                            r.reply_to_name.clone()
                        },
                        r.text
                    ))))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .text_xs()
                            .child(div().text_color(theme.muted_foreground).child(
                                SharedString::from(crate::home::fmt::relative_time(r.create_time)),
                            ))
                            .child(
                                h_flex()
                                    .id(ElementId::from(format!("reply-digg-{rid}")))
                                    .items_center()
                                    .gap_1()
                                    .cursor_pointer()
                                    .text_color(if digged {
                                        hot_color(theme.mode).into()
                                    } else {
                                        theme.muted_foreground
                                    })
                                    .hover(|s| s.text_color(theme.foreground))
                                    .on_click({
                                        let (p, rid) = (parent_id.clone(), rid.clone());
                                        cx.listener(move |this, _, window, cx| {
                                            this.toggle_review_reply_digg(
                                                p.clone(),
                                                rid.clone(),
                                                window,
                                                cx,
                                            );
                                        })
                                    })
                                    .child(Icon::new(gpui_kit::assets::IconName::Heart).size_3())
                                    .children((digg_count > 0).then(|| {
                                        div().child(SharedString::from(digg_count.to_string()))
                                    })),
                            )
                            .child({
                                let (p, rid, name) =
                                    (parent_id.clone(), rid.clone(), r.user_name.clone());
                                div()
                                    .id(ElementId::from(format!("reply-to-{rid}")))
                                    .cursor_pointer()
                                    .text_color(theme.muted_foreground)
                                    .hover(|s| s.text_color(theme.foreground))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_review_reply_target(
                                            p.clone(),
                                            Some((rid.clone(), name.clone())),
                                            cx,
                                        );
                                    }))
                                    .child(crate::i18n::t("detail.reply"))
                            })
                            .children(mine.then(|| {
                                let (p, rid) = (parent_id.clone(), rid.clone());
                                div()
                                    .id(ElementId::from(format!("reply-del-{rid}")))
                                    .cursor_pointer()
                                    .text_color(theme.muted_foreground)
                                    .hover(|s| s.text_color(theme.danger))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.confirm_delete_review_item(
                                            rid.clone(),
                                            4,
                                            Some(p.clone()),
                                            window,
                                            cx,
                                        );
                                    }))
                                    .child(crate::i18n::t("common.delete"))
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 发评框（Tauri ReviewComposer 同款：我的评分五星 + 表情 + 文本 +
    /// 「发表剧评」；未选评分或正文为空禁发；Enter 提交见 mod.rs 的订阅）。
    pub(super) fn render_review_composer(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let shown = if self.review_hover_stars > 0 {
            self.review_hover_stars
        } else {
            self.review_stars
        };
        let has_text = self
            .review_input
            .as_ref()
            .is_some_and(|i| !i.read(cx).value().trim().is_empty());
        let can_send = has_text && self.review_stars >= 1 && !self.review_sending;
        let mut stars = h_flex().items_center().gap_2().child(
            div()
                .text_sm()
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .child(crate::i18n::t("detail.myRating")),
        );
        for s in 1..=5u8 {
            stars = stars.child(
                div()
                    .id(ElementId::from(format!("review-star-{s}")))
                    .p_0p5()
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        let next = if *hovered {
                            s
                        } else if this.review_hover_stars == s {
                            0
                        } else {
                            this.review_hover_stars
                        };
                        if this.review_hover_stars != next {
                            this.review_hover_stars = next;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.review_stars = s;
                        this.review_hover_stars = 0;
                        cx.notify();
                    }))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::Star)
                            .size_5()
                            .text_color(if s <= shown {
                                star_color(theme.mode).into()
                            } else {
                                theme.muted_foreground
                            }),
                    ),
            );
        }
        stars = stars.children((self.review_stars > 0).then(|| {
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(crate::i18n::t("detail.ratedSuffix"))
        }));
        let mut card = v_flex()
            .rounded(gpui_kit::px(8.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.group_box)
            .p_3()
            .gap_2()
            .child(stars)
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("review-emoji-btn")
                            .px_1()
                            .cursor_pointer()
                            .text_color(if self.review_emoji_open {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .hover(|s| s.text_color(theme.foreground))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.review_emoji_open = !this.review_emoji_open;
                                cx.notify();
                            }))
                            .child(Icon::new(gpui_kit::assets::IconName::FaceGrinning).size_4()),
                    )
                    .children(self.review_input.as_ref().map(|input| {
                        // 多行文本域（Tauri ReviewComposer Textarea 同款）：
                        // 无边框融入卡片，3 行高
                        gpui_kit::component::input::Textarea::new(input)
                            .flex_1()
                            .h(gpui_kit::px(72.0))
                            .text_sm()
                            .appearance(false)
                            .bordered(false)
                    }))
                    .child(
                        Button::new("review-send")
                            .label(if self.review_sending {
                                crate::i18n::t("common.sending")
                            } else {
                                crate::i18n::t("detail.reviewSend")
                            })
                            .small()
                            .disabled(!can_send)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.submit_review(window, cx);
                            })),
                    ),
            );
        if self.review_emoji_open {
            // 表情网格（8 列，与评论面板同一套资产；点格子光标处插入
            // `[名字]`，连选不关）
            card = card.child(
                div()
                    .id("review-emoji-grid")
                    .max_h(gpui_kit::px(132.0))
                    .overflow_y_scroll()
                    .flex()
                    .flex_wrap()
                    .gap_0p5()
                    .children(crate::danmaku_emoji::EMOJI_FILES.iter().map(|(name, _)| {
                        let code = name.to_string();
                        let img = crate::danmaku_emoji::emoji_by_name(name);
                        let input = self.review_input.clone();
                        div()
                            .id(ElementId::from(format!("review-emoji-{code}")))
                            .size_7()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(gpui_kit::px(6.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.secondary_hover))
                            .on_click(move |_, window, cx| {
                                if let Some(input) = &input {
                                    input.update(cx, |state, cx| {
                                        state.insert(code.clone(), window, cx);
                                    });
                                }
                            })
                            .children(img.map(|img| {
                                gpui_kit::img(gpui_kit::ImageSource::Render(img))
                                    .size_5()
                                    .object_fit(gpui_kit::ObjectFit::Contain)
                            }))
                    })),
            );
        }
        card.into_any_element()
    }

    pub(crate) fn render_reviews(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        v_flex()
            .mt_4()
            .gap_4()
            // 发评框（Tauri ReviewComposer 同位：列表在它下面）
            .child(self.render_review_composer(cx))
            // 评分摘要卡（Tauri ReviewList 同款：边框卡里 大字评分 + 五星 +
            // 人数 + 标签统计胶囊；无评分时退化为五星占位 + 暂无评分）
            .child({
                let has_score = !self.reviews_score.is_empty();
                let star = crate::tokens::star(theme.mode);
                div()
                    .rounded(gpui_kit::px(8.0))
                    .border_1()
                    .border_color(theme.border)
                    .px_4()
                    .py_3()
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_3()
                            .children(has_score.then(|| {
                                h_flex()
                                    .items_baseline()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(gpui_kit::px(30.0))
                                            .line_height(gpui_kit::relative(1.0))
                                            .font_weight(gpui_kit::FontWeight::BOLD)
                                            .text_color(star)
                                            .child(format!(
                                                "{:.1}",
                                                self.reviews_score.parse::<f64>().unwrap_or(0.0)
                                            )),
                                    )
                                    .child(score_stars(&self.reviews_score, theme))
                            }))
                            .children((!has_score).then(|| score_stars("0", theme)))
                            .child(div().text_sm().text_color(theme.muted_foreground).child(
                                if has_score && self.reviews_score_cnt > 0 {
                                    crate::i18n::tf(
                                        "detail.ratingCount",
                                        &[(
                                            "count",
                                            &DetailView::fmt_count(self.reviews_score_cnt),
                                        )],
                                    )
                                } else if has_score {
                                    crate::i18n::t("detail.avgRating")
                                } else {
                                    crate::i18n::t("detail.noRating")
                                },
                            ))
                            .children(self.reviews_tag_stats.iter().map(|tag| {
                                div()
                                    .rounded_full()
                                    .bg(theme.muted)
                                    .px_3()
                                    .py_1()
                                    .text_xs()
                                    .text_color(theme.foreground)
                                    .child(SharedString::from(format!(
                                        "{} {}",
                                        tag.tag_name, tag.count
                                    )))
                            })),
                    )
            })
            // 剧评列表（Tauri ReviewList 同款：头像 + 昵称 + 评分 + 时间 +
            // 正文 + 点赞/回复/删除 + 回复线程（懒加载）与行内回复框；
            // 评分行只有剧评有——单集评论 score 恒空）
            .children(
                self.reviews
                    .iter()
                    .map(|c| {
                        let avatar = self.images.get(&c.avatar).cloned();
                        let rid = c.comment_id.clone();
                        let is_mine =
                            !self.my_user_id().is_empty() && c.user_id == self.my_user_id();
                        let digged = self
                            .review_digg_override
                            .get(&rid)
                            .copied()
                            .unwrap_or(c.user_digg);
                        let digg_count = c.digg_count;
                        let reply_count = c.reply_count;
                        let replies_open = self.review_replies_open.contains(&rid);
                        let replies = self
                            .review_replies
                            .get(&rid)
                            .map(|v| (**v).clone())
                            .unwrap_or_default();
                        let loading = self.review_replies_loading.contains(&rid);
                        let has_more = self.review_replies_cursor.contains_key(&rid);
                        let reply_target_open = self
                            .review_reply_target
                            .as_ref()
                            .is_some_and(|(r, _)| r == &rid);
                        let reply_to_reply = self
                            .review_reply_target
                            .as_ref()
                            .filter(|(r, _)| r == &rid)
                            .and_then(|(_, t)| t.clone());
                        let reply_input = self.review_reply_input.clone();
                        v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .items_start()
                            .gap_3()
                            .child(
                                div()
                                    .size_9()
                                    .flex_shrink_0()
                                    .overflow_hidden()
                                    .rounded_full()
                                    .bg(theme.secondary)
                                    .children(avatar.map(|bitmap| {
                                        img(ImageSource::Render(bitmap))
                                            .size_full()
                                            .object_fit(ObjectFit::Cover)
                                    })),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                                            .child(SharedString::from(c.user_name.clone())),
                                    )
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_2()
                                            // Tauri ReviewList 同款：五星 + 绝对日期 +
                                            // 评分后缀文案（「观看N小时后点评」）
                                            .children((!c.score.is_empty()).then(|| {
                                                score_stars(&c.score, theme)
                                            }))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(theme.muted_foreground)
                                                    .child(SharedString::from(review_date(
                                                        c.create_time,
                                                    ))),
                                            )
                                            .children(
                                                (!c.score_suffix_text.is_empty()).then(|| {
                                                    div()
                                                        .text_xs()
                                                        .text_color(theme.muted_foreground)
                                                        .child(SharedString::from(
                                                            c.score_suffix_text.clone(),
                                                        ))
                                                }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .line_height(gpui_kit::relative(1.6))
                                            .child(render_review_text(&c.text)),
                                    )
                                    // 动作行（Tauri ReviewList 同款：♥点赞 /
                                    // 回复 / 删除（自己的）/ 展开回复）
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_4()
                                            .text_xs()
                                            .child(
                                                h_flex()
                                                    .id(ElementId::from(format!(
                                                        "review-digg-{rid}"
                                                    )))
                                                    .items_center()
                                                    .gap_1()
                                                    .cursor_pointer()
                                                    .text_color(if digged {
                                                        hot_color(theme.mode).into()
                                                    } else {
                                                        theme.muted_foreground
                                                    })
                                                    .hover(|s| s.text_color(theme.foreground))
                                                    .on_click({
                                                        let rid = rid.clone();
                                                        cx.listener(move |this, _, window, cx| {
                                                            this.toggle_review_digg(
                                                                rid.clone(),
                                                                window,
                                                                cx,
                                                            );
                                                        })
                                                    })
                                                    .child(
                                                        Icon::new(
                                                            gpui_kit::assets::IconName::Heart,
                                                        )
                                                        .size_3p5(),
                                                    )
                                                    .children((digg_count > 0).then(|| {
                                                        div().child(SharedString::from(
                                                            digg_count.to_string(),
                                                        ))
                                                    })),
                                            )
                                            .child(
                                                div()
                                                    .id(ElementId::from(format!(
                                                        "review-reply-{rid}"
                                                    )))
                                                    .cursor_pointer()
                                                    .text_color(theme.muted_foreground)
                                                    .hover(|s| s.text_color(theme.foreground))
                                                    .on_click({
                                                        let rid = rid.clone();
                                                        cx.listener(move |this, _, _, cx| {
                                                            this.set_review_reply_target(
                                                                rid.clone(),
                                                                None,
                                                                cx,
                                                            );
                                                        })
                                                    })
                                                    .child(crate::i18n::t("detail.reply")),
                                            )
                                            .children(is_mine.then(|| {
                                                let rid = rid.clone();
                                                div()
                                                    .id(ElementId::from(format!(
                                                        "review-del-{rid}"
                                                    )))
                                                    .cursor_pointer()
                                                    .text_color(theme.muted_foreground)
                                                    .hover(|s| s.text_color(theme.danger))
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            this.confirm_delete_review_item(
                                                                rid.clone(),
                                                                2,
                                                                None,
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    ))
                                                    .child(crate::i18n::t("common.delete"))
                                            }))
                                            .children((reply_count > 0).then(|| {
                                                let rid = rid.clone();
                                                div()
                                                    .id(ElementId::from(format!(
                                                        "review-replies-toggle-{rid}"
                                                    )))
                                                    .cursor_pointer()
                                                    .text_color(theme.muted_foreground)
                                                    .hover(|s| s.text_color(theme.foreground))
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.toggle_review_replies(
                                                                rid.clone(),
                                                                reply_count,
                                                                cx,
                                                            );
                                                        },
                                                    ))
                                                    .child(if replies_open {
                                                        crate::i18n::t(
                                                            "detail.collapseReplies",
                                                        )
                                                    } else {
                                                        crate::i18n::tf(
                                                            "detail.expandReplies",
                                                            &[("n", &reply_count.to_string())],
                                                        )
                                                    })
                                            })),
                                    ),
                            ),
                    )
                    // 回复线程（懒加载；行 = 头像/昵称/正文/♥/回复/删除）
                    .children(replies_open.then(|| {
                        let mut col = v_flex().pl_12().gap_2();
                        if loading && replies.is_empty() {
                            col = col.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(crate::i18n::t("common.loading")),
                            );
                        }
                        for r in &replies {
                            col = col.child(self.render_review_reply_row(&rid, r, cx));
                        }
                        if has_more {
                            col = col.child(
                                div()
                                    .id(ElementId::from(format!("review-more-{rid}")))
                                    .text_xs()
                                    .cursor_pointer()
                                    .text_color(theme.muted_foreground)
                                    .hover(|s| s.text_color(theme.foreground))
                                    .child(if loading {
                                        crate::i18n::t("common.loading")
                                    } else {
                                        crate::i18n::t("detail.moreReplies")
                                    })
                                    .on_click({
                                        let rid = rid.clone();
                                        let cursor = self
                                            .review_replies_cursor
                                            .get(&rid)
                                            .cloned()
                                            .unwrap_or_default();
                                        cx.listener(move |this, _, _, cx| {
                                            this.fetch_review_replies(
                                                rid.clone(),
                                                cursor.clone(),
                                                cx,
                                            );
                                        })
                                    }),
                            );
                        }
                        col
                    }))
                    // 行内回复框（Tauri replyTarget 同位：mt-2 小输入 + 回复）
                    .children((reply_target_open && reply_input.is_some()).then(|| {
                        let input = reply_input.clone().unwrap();
                        h_flex()
                            .pl_12()
                            .mt_1()
                            .items_center()
                            .gap_2()
                            .child(
                                Input::new(&input)
                                    .flex_1()
                                    .h(gpui_kit::px(32.0))
                                    .text_xs(),
                            )
                            .child(
                                Button::new(ElementId::from(format!(
                                    "review-reply-send-{rid}"
                                )))
                                .label(if self.review_reply_sending {
                                    crate::i18n::t("common.sending")
                                } else {
                                    crate::i18n::t("detail.reply")
                                })
                                .small()
                                .disabled(
                                    input.read(cx).value().trim().is_empty()
                                        || self.review_reply_sending,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.submit_review_reply(window, cx);
                                })),
                            )
                            .child(
                                Button::new(ElementId::from(format!(
                                    "review-reply-cancel-{rid}"
                                )))
                                .label(crate::i18n::t("common.cancel"))
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.review_reply_target = None;
                                    cx.notify();
                                })),
                            )
                            .children(reply_to_reply.as_ref().map(|(_, name)| {
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(SharedString::from(format!(
                                        "{} @{name}",
                                        crate::i18n::t("detail.replyToPrefix")
                                    )))
                            }))
                    }))
                    .into_any_element()
                    })
                    .collect::<Vec<_>>(),
            )
            .children(self.reviews.is_empty().then(|| {
                div()
                    .py_6()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(if self.reviews_loading {
                        crate::i18n::t("common.loading")
                    } else {
                        crate::i18n::t("detail.reviewsEmpty")
                    })
            }))
            // 加载更多（Tauri 是滚动懒加载；GPUI 详情页滚动容器裁判复杂，
            // 按钮等价且省一档哨兵）
            .children(
                (self.reviews_has_more && !self.reviews.is_empty()).then(|| {
                    Button::new("detail-reviews-more")
                        .label(if self.reviews_loading {
                            crate::i18n::t("common.loading")
                        } else {
                            crate::i18n::t("detail.loadMoreReviews")
                        })
                        .secondary()
                        .small()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_reviews_next(cx);
                        }))
                }),
            )
            .into_any_element()
    }
}

/// 十分制评分 → 五星（Tauri `ScoreStars` 同款：整数星实心、余下 ≥0.5 半星
/// 用 50% 透明实心星、空位用描边星）。
fn score_stars(score: &str, theme: &gpui_kit::component::Theme) -> gpui_kit::AnyElement {
    let value = (score.parse::<f64>().unwrap_or(0.0) / 2.0).clamp(0.0, 5.0);
    let filled = value.floor() as u32;
    let half = value - value.floor() >= 0.5;
    let star = crate::tokens::star(theme.mode);
    h_flex()
        .items_center()
        .gap(gpui_kit::px(1.0))
        .children((1..=5u32).map(|n| {
            if n <= filled {
                Icon::new(gpui_kit::assets::IconName::StarFill)
                    .size_3()
                    .text_color(star)
                    .into_any_element()
            } else if n == filled + 1 && half {
                Icon::new(gpui_kit::assets::IconName::StarFill)
                    .size_3()
                    .text_color(star.opacity(0.5))
                    .into_any_element()
            } else {
                Icon::new(gpui_kit::assets::IconName::Star)
                    .size_3()
                    .text_color(theme.muted_foreground.opacity(0.4))
                    .into_any_element()
            }
        }))
        .into_any_element()
}

/// 剧评时间 → 绝对日期（Tauri `toLocaleDateString()` 同款，`2026/10/15`）。
fn review_date(ts_ms: i64) -> String {
    if ts_ms <= 0 {
        return String::new();
    }
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ts_ms)
        .single()
        .map(|dt| dt.format("%Y/%m/%d").to_string())
        .unwrap_or_default()
}

fn render_review_text(text: &str) -> gpui_kit::AnyElement {
    let mut row = div().flex().flex_wrap().items_center();
    for seg in crate::danmaku_emoji::parse_emoji_segments(text) {
        row = row.child(match seg {
            crate::danmaku_emoji::EmojiSegment::Text(t) => {
                div().child(SharedString::from(t)).into_any_element()
            }
            crate::danmaku_emoji::EmojiSegment::Emoji(img) => {
                gpui_kit::img(gpui_kit::ImageSource::Render(img))
                    .size(gpui_kit::px(18.0))
                    .object_fit(gpui_kit::ObjectFit::Contain)
                    .into_any_element()
            }
        });
    }
    row.into_any_element()
}

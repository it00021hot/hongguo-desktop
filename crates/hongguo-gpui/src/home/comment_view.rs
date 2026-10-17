//! 评论/回复面板的渲染（列表、回复行、删除确认浮层）：数据操作在
//! super::comments，本文件只管画。

#[cfg(feature = "screenshot")]
use std::time::{Duration, Instant};

use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, SharedString, div};
use hongguo_api::danmaku::CommentItem;

#[cfg(feature = "screenshot")]
use super::controls::CONTROLS_HOLD_SECS;

impl super::HomeView {
    /// 评论面板（Tauri comment-panel 对齐：右侧滑出，列表 + 发送框；
    /// 回复内联展开；点赞乐观 ±1；表情代码随表情基建对齐补）。
    pub(super) fn render_comment_panel(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let loading = self.comments_loading && self.comments.is_empty();
        let comment_input = self.comment_input.clone();
        // @提及候选弹层开关（Tauri mentionOpen 同款：输入以 `@词` 结尾即弹）
        let mention_open = comment_input
            .as_ref()
            .map(|i| mention_query_open(&i.read(cx).value()))
            .unwrap_or(false);
        let reply_target = self
            .reply_to
            .as_ref()
            .map(|(_, _, name)| name.clone())
            .unwrap_or_default();

        v_flex()
            .id("comment-panel")
            .absolute()
            .right_0()
            // Tauri comment-panel 的几何同款：bottom-14（56px，让开控制条）
            // + h-[68%]。此前铺到舞台底、输入行被控制条压住点不到。
            .bottom(gpui_kit::px(56.0))
            .h(gpui_kit::relative(0.68))
            .w(gpui_kit::px(380.0))
            .bg(gpui_kit::rgba(0x171717f2))
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            // 滚轮拦下（Tauri comment-panel data-wheel-block 同款）：面板内
            // 滚动是列表自己在滚，不冒泡成舞台切剧
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
            .child(
                // 头部：评论 · N + 关闭
                h_flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(gpui_kit::rgb(0xffffff))
                            .child(format!(
                                "{} · {}",
                                crate::i18n::t("player.comments.title"),
                                self.comments_total
                            )),
                    )
                    .child(
                        Button::new("comment-close")
                            .icon(gpui_kit::assets::IconName::X)
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.comments_panel_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("comments-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_4()
                    .child(if loading {
                        div()
                            .py_8()
                            .text_center()
                            .text_sm()
                            .text_color(gpui_kit::rgba(0xffffff99))
                            .child(crate::i18n::t("common.loading"))
                            .into_any_element()
                    } else if self.comments.is_empty() {
                        div()
                            .py_8()
                            .text_center()
                            .text_sm()
                            .text_color(gpui_kit::rgba(0xffffff99))
                            .child(crate::i18n::t("player.comments.empty"))
                            .into_any_element()
                    } else {
                        v_flex()
                            .gap_4()
                            .children(self.comments.iter().map(|c| self.render_comment_row(c, cx)))
                            .children(self.comments_has_more.then(|| {
                                div()
                                    .id("comments-more")
                                    .py_2()
                                    .text_center()
                                    .text_xs()
                                    .cursor_pointer()
                                    .text_color(gpui_kit::rgba(0x60a5faff))
                                    .hover(|s| s.text_color(gpui_kit::rgb(0x93c5fd)))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.load_comments_next(cx);
                                    }))
                                    .child(if self.comments_loading {
                                        crate::i18n::t("common.loading")
                                    } else {
                                        crate::i18n::tf(
                                            "player.comments.loadMore",
                                            &[(
                                                "n",
                                                &self
                                                    .comments_total
                                                    .saturating_sub(self.comments.len() as i64)
                                                    .to_string(),
                                            )],
                                        )
                                    })
                            }))
                            .into_any_element()
                    }),
            )
            .child(
                // 底部发送框：回复目标指示 + 输入 + 发送
                v_flex()
                    .px_4()
                    .py_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(gpui_kit::rgba(0x262626ff))
                    // @提及候选弹层（Tauri CommentPanel 同款：横排候选 +
                    // AI 头像角标 + 「Ai」后缀；点选→延迟写入输入框）。
                    // 表情选择器/候选都在输入行上方；无候选时给提示行。
                    .children(mention_open.then(|| {
                        let candidates: Vec<gpui_kit::AnyElement> = if self.mention_pending {
                            vec![
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .px_3()
                                    .py_2p5()
                                    .child(gpui_kit::component::spinner::Spinner::new().small())
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(gpui_kit::rgba(0xffffff99))
                                            .child(crate::i18n::t(
                                                "player.comments.mentionLoading",
                                            )),
                                    )
                                    .into_any_element(),
                            ]
                        } else if self.mention_users.is_empty() {
                            vec![
                                div()
                                    .px_3()
                                    .py_2p5()
                                    .text_xs()
                                    .text_color(gpui_kit::rgba(0xffffff99))
                                    .child(if self.logged_in() {
                                        crate::i18n::t("player.comments.mentionUnavailable")
                                    } else {
                                        crate::i18n::t("player.interact.loginRequired")
                                    })
                                    .into_any_element(),
                            ]
                        } else {
                            self.mention_users
                                .iter()
                                .map(|u| {
                                    let (uid, name, avatar, bot) = (
                                        u.user_id.clone(),
                                        u.user_name.clone(),
                                        u.avatar.clone(),
                                        u.is_ai_bot,
                                    );
                                    let click_name = name.clone();
                                    v_flex()
                                        .id(ElementId::from(format!("mention-{uid}")))
                                        .w(gpui_kit::px(56.0))
                                        .items_center()
                                        .gap_1()
                                        .rounded(gpui_kit::px(6.0))
                                        .p_1()
                                        .cursor_pointer()
                                        .hover(|s| s.bg(gpui_kit::rgba(0xffffff1a)))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            // 选中 = 记提及对象 + 排一段待插入
                                            // 文本（render 期 set_value，见
                                            // home/mod.rs 的延迟写入）
                                            this.mention_user =
                                                Some((uid.clone(), click_name.clone()));
                                            this.mention_insert =
                                                Some(format!("@{click_name}\u{FFFC} "));
                                            cx.notify();
                                        }))
                                        .child(
                                            div()
                                                .relative()
                                                .child(self.render_avatar(&avatar, &name, 44.0))
                                                .children(bot.then(|| {
                                                    // AI 角标：橙底白勾（Tauri 同位）
                                                    div()
                                                        .absolute()
                                                        .right(gpui_kit::px(-2.0))
                                                        .bottom(gpui_kit::px(-2.0))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .size(gpui_kit::px(16.0))
                                                        .rounded_full()
                                                        .bg(gpui_kit::rgb(0xf97316))
                                                        .child(
                                                            Icon::new(
                                                                gpui_kit::assets::IconName::Check,
                                                            )
                                                            .with_size(gpui_kit::px(10.0))
                                                            .text_color(gpui_kit::rgb(0xffffff)),
                                                        )
                                                })),
                                        )
                                        .child(
                                            h_flex()
                                                .items_center()
                                                .gap_0p5()
                                                .text_size(gpui_kit::px(11.0))
                                                .text_color(gpui_kit::rgba(0xffffffe6))
                                                .child(
                                                    div()
                                                        .truncate()
                                                        .child(SharedString::from(name.clone())),
                                                )
                                                .children(bot.then(|| {
                                                    div()
                                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                                        .text_color(gpui_kit::rgb(0xfbbf24))
                                                        .child("Ai")
                                                })),
                                        )
                                        .into_any_element()
                                })
                                .collect()
                        };
                        h_flex()
                            .id("mention-popup")
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .rounded(gpui_kit::px(8.0))
                            .border_1()
                            .border_color(gpui_kit::rgba(0xffffff1a))
                            .bg(gpui_kit::rgb(0x262626))
                            .p_2()
                            .children(candidates)
                    }))
                    .children((!reply_target.is_empty()).then(|| {
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(gpui_kit::rgba(0x93c5fdff))
                                    .child(format!(
                                        "{} @{reply_target}：",
                                        crate::i18n::t("player.comments.replyToPrefix")
                                    )),
                            )
                            .child(
                                div()
                                    .id("reply-cancel")
                                    .cursor_pointer()
                                    .text_xs()
                                    .text_color(gpui_kit::rgba(0xffffff99))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reply_to = None;
                                        cx.notify();
                                    }))
                                    .child(crate::i18n::t("common.cancel")),
                            )
                    }))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            // 表情按钮（Tauri EmojiPickerButton 同款：开时高亮；
                            // 点击不夺输入框焦点，插入落真实光标处）
                            .child(
                                div()
                                    .id("emoji-btn")
                                    .px_1()
                                    .cursor_pointer()
                                    .text_color(if self.emoji_picker_open {
                                        gpui_kit::rgb(0xffffff)
                                    } else {
                                        gpui_kit::rgba(0xffffff99)
                                    })
                                    .hover(|s| s.text_color(gpui_kit::rgb(0xffffff)))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.emoji_picker_open = !this.emoji_picker_open;
                                        cx.notify();
                                    }))
                                    .child(
                                        Icon::new(gpui_kit::assets::IconName::FaceGrinning)
                                            .size_4(),
                                    ),
                            )
                            .children(comment_input.as_ref().map(|input| {
                                Input::new(input).flex_1().h(gpui_kit::px(32.0)).text_xs()
                            }))
                            .child(
                                Button::new("comment-send")
                                    .label(if self.reply_to.is_some() {
                                        crate::i18n::t("player.comments.reply")
                                    } else {
                                        crate::i18n::t("player.interact.send")
                                    })
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if this.reply_to.is_some() {
                                            this.submit_reply(cx);
                                        } else {
                                            this.submit_comment(cx);
                                        }
                                    })),
                            ),
                    ),
            )
            // 表情选择器：点外部收起的捕获层 + 上拉网格（Tauri EmojiPicker
            // 同款：8 列、每格一张表情图、连选不关面板、选完光标处插入）
            .children(self.emoji_picker_open.then(|| {
                div()
                    .absolute()
                    .inset_0()
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.emoji_picker_open = false;
                            cx.notify();
                        }),
                    )
                    .into_any_element()
            }))
            .children(self.emoji_picker_open.then(|| {
                let comment_input = comment_input.clone();
                v_flex()
                    .absolute()
                    .left_3()
                    .bottom(gpui_kit::px(56.0))
                    .w(gpui_kit::px(264.0))
                    .p_2()
                    .rounded(gpui_kit::px(12.0))
                    .bg(gpui_kit::rgba(0x000000d9))
                    .border_1()
                    .border_color(gpui_kit::rgba(0xffffff14))
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    )
                    .child(
                        div()
                            .id("emoji-grid")
                            .max_h(gpui_kit::px(176.0))
                            .overflow_y_scroll()
                            .flex()
                            .flex_wrap()
                            .gap_0p5()
                            .children(crate::danmaku_emoji::EMOJI_FILES.iter().map(|(name, _)| {
                                let code = name.to_string();
                                let img = crate::danmaku_emoji::emoji_by_name(name);
                                div()
                                    .id(gpui_kit::ElementId::from(format!("emoji-{}", code)))
                                    .size_8()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(gpui_kit::px(6.0))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(gpui_kit::rgba(0xffffff26)))
                                    .on_click({
                                        let input = comment_input.clone();
                                        move |_, window, cx| {
                                            if let Some(input) = &input {
                                                // 光标处插入 [名字]（焦点纪律：
                                                // 点格子不夺焦，选完接着打字）
                                                input.update(cx, |state, cx| {
                                                    state.insert(code.clone(), window, cx);
                                                });
                                            }
                                        }
                                    })
                                    .children(img.map(|img| {
                                        gpui_kit::img(gpui_kit::ImageSource::Render(img))
                                            .size_6()
                                            .object_fit(gpui_kit::ObjectFit::Contain)
                                    }))
                            })),
                    )
                    .into_any_element()
            }))
            .children(self.comment_delete_target.is_some().then(|| {
                // 删除确认浮层（Tauri ConfirmDialog 同款语义：comment/del
                // service_id=4，评论与回复同款；点遮罩=取消）
                let is_reply = self
                    .comment_delete_target
                    .as_ref()
                    .and_then(|(_, p)| p.as_ref())
                    .is_some();
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui_kit::rgba(0x00000099))
                    .on_mouse_down(
                        gpui_kit::MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.cancel_delete(cx)),
                    )
                    .child(
                        v_flex()
                            .w(gpui_kit::px(272.0))
                            .p_4()
                            .gap_2p5()
                            .rounded(gpui_kit::px(12.0))
                            .bg(gpui_kit::rgb(0x262626))
                            .on_mouse_down(
                                gpui_kit::MouseButton::Left,
                                cx.listener(|_, _, _, cx| cx.stop_propagation()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                    .text_color(gpui_kit::rgb(0xffffff))
                                    .child(if is_reply {
                                        crate::i18n::t("player.comments.deleteReplyTitle")
                                    } else {
                                        crate::i18n::t("player.comments.deleteCommentTitle")
                                    }),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(gpui_kit::rgba(0xffffff99))
                                    .child(crate::i18n::t("player.comments.deleteDesc")),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_end()
                                    .gap_2()
                                    .mt_1()
                                    .child(
                                        Button::new("comment-del-cancel")
                                            .label(crate::i18n::t("common.cancel"))
                                            .ghost()
                                            .small()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.cancel_delete(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("comment-del-confirm")
                                            .label(if self.comment_deleting {
                                                crate::i18n::t("batch.deleting")
                                            } else {
                                                crate::i18n::t("common.delete")
                                            })
                                            .danger()
                                            .small()
                                            .disabled(self.comment_deleting)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.confirm_delete(cx);
                                            })),
                                    ),
                            ),
                    )
            }))
            .into_any_element()
    }

    /// 评论列表一行：头像占位（昵称首字圆形）/昵称/文本/相对时间/♡。
    /// 评论正文：`@名字\u{FFFC}` 提及前缀染蓝（AI bot 名字补琥珀「Ai」
    /// 后缀——Tauri 的渐变文字在 GPUI 用单色近似），其余按 `[名字]`
    /// 表情分段内联（与弹幕共用一套 parse）。面板属 hgplayer 皮肤域，
    /// 固定色值属硬红线 11 的播放器豁免。
    fn render_comment_text(&self, text: &str) -> gpui_kit::AnyElement {
        let ai_name = self
            .mention_users
            .iter()
            .find(|u| u.is_ai_bot)
            .map(|u| u.user_name.clone())
            .unwrap_or_else(|| crate::i18n::t("player.comments.aiName"));
        let (prefix, rest) = match parse_mention_prefix(text) {
            Some((name, rest)) => (Some(name), rest),
            None => (None, text),
        };
        let mut row = div().flex().flex_wrap().items_center();
        if let Some(name) = prefix {
            row = row.child(
                h_flex()
                    .items_center()
                    .mr_1()
                    .text_color(gpui_kit::rgb(0x38bdf8))
                    .child(SharedString::from(format!("@{name}")))
                    .children((name == ai_name).then(|| {
                        div()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(gpui_kit::rgb(0xfbbf24))
                            .child("Ai")
                    })),
            );
        }
        for seg in crate::danmaku_emoji::parse_emoji_segments(rest) {
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

    fn render_comment_row(&self, c: &CommentItem, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let cid = c.comment_id.clone();
        let is_mine = !self.my_user_id().is_empty() && c.user_id == self.my_user_id();
        let digged = self
            .comment_digg_override
            .get(&cid)
            .copied()
            .unwrap_or(c.user_digg);
        let digg_count = c.digg_count;
        let replies = self.replies.get(&cid).cloned();
        let replies_open = self.replies_open.contains(&cid);
        let reply_count = c.reply_count;
        let loading = self.replies_loading.contains(&cid);
        let my_reply_target = self.reply_to.as_ref().is_some_and(|(rc, _, _)| rc == &cid);
        let _ = my_reply_target;

        v_flex()
            .gap_1p5()
            .child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(self.render_avatar(&c.avatar, &c.user_name, 32.0))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(gpui_kit::rgba(0xffffff99))
                                    .child(SharedString::from(if c.user_name.is_empty() {
                                        crate::i18n::t("player.comments.anon")
                                    } else {
                                        c.user_name.clone()
                                    })),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(gpui_kit::rgba(0xffffffe6))
                                    .child(self.render_comment_text(&c.text)),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_size(gpui_kit::px(11.0))
                                            .text_color(gpui_kit::rgba(0xffffff73))
                                            .child(crate::home::fmt::relative_time(c.create_time)),
                                    )
                                    .child(
                                        // 回复本条评论（打开底部回复输入框；
                                        // Tauri 评论行「回复」同款）
                                        div()
                                            .id(ElementId::from(format!("comment-reply-{cid}")))
                                            .text_size(gpui_kit::px(11.0))
                                            .cursor_pointer()
                                            .text_color(gpui_kit::rgba(0xffffff73))
                                            .hover(|s| s.text_color(gpui_kit::rgba(0xd4d4d8ff)))
                                            .on_click({
                                                let cid = cid.clone();
                                                let name = c.user_name.clone();
                                                cx.listener(move |this, _, _, cx| {
                                                    // 只展开底部回复输入框（Tauri
                                                    // setReplyTarget 同款，不拉回复区）
                                                    this.reply_to =
                                                        Some((cid.clone(), None, name.clone()));
                                                    cx.notify();
                                                })
                                            })
                                            .child(crate::i18n::t("player.comments.reply")),
                                    )
                                    .children(is_mine.then(|| {
                                        div()
                                            .id(ElementId::from(format!("comment-del-{cid}")))
                                            .text_size(gpui_kit::px(11.0))
                                            .cursor_pointer()
                                            .text_color(gpui_kit::rgba(0xffffff73))
                                            .hover(|s| s.text_color(gpui_kit::rgb(0xf87171)))
                                            .on_click({
                                                let cid = cid.clone();
                                                cx.listener(move |this, _, _, cx| {
                                                    this.request_delete(cid.clone(), None, cx);
                                                })
                                            })
                                            .child(crate::i18n::t("common.delete"))
                                    }))
                                    .children((reply_count > 0).then({
                                        let cid = cid.clone();
                                        move || {
                                            div()
                                                .id(ElementId::from(format!("reply-toggle-{cid}")))
                                                .text_size(gpui_kit::px(11.0))
                                                .cursor_pointer()
                                                .text_color(gpui_kit::rgba(0x60a5faff))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.toggle_replies(
                                                        cid.clone(),
                                                        reply_count,
                                                        cx,
                                                    );
                                                }))
                                                .child(if replies_open {
                                                    crate::i18n::t(
                                                        "player.comments.collapseReplies",
                                                    )
                                                } else {
                                                    crate::i18n::tf(
                                                        "player.comments.expandReplies",
                                                        &[("n", &reply_count.to_string())],
                                                    )
                                                })
                                        }
                                    })),
                            ),
                    )
                    .child(
                        // ♡ 点赞（乐观 ±1）
                        div()
                            .id(ElementId::from(format!("comment-digg-{cid}")))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_0p5()
                            .cursor_pointer()
                            .text_color(if digged {
                                gpui_kit::rgb(0xef4444)
                            } else {
                                gpui_kit::rgba(0xffffff99)
                            })
                            .on_click({
                                let cid = cid.clone();
                                cx.listener(move |this, _, _, cx| {
                                    this.toggle_comment_digg(cid.clone(), cx);
                                })
                            })
                            .child(Icon::new(gpui_kit::assets::IconName::Heart).size_4())
                            .children((digg_count > 0).then(|| {
                                div()
                                    .text_size(gpui_kit::px(10.0))
                                    .child(format!("{digg_count}"))
                            })),
                    ),
            )
            .children(replies_open.then(|| {
                let items = replies.map(|v| (*v).clone()).unwrap_or_default();
                v_flex()
                    .pl_10()
                    .gap_2()
                    // 「思考中…」占位（Tauri pendingAi 同款）：@小果 提及评论
                    // 发出后回复区强制展开，轮询到 AI 回复即撤
                    .children(self.ai_pending.iter().any(|(id, _)| id == &cid).then(|| {
                        div()
                            .text_xs()
                            .text_color(gpui_kit::rgba(0xffffff73))
                            .child(crate::i18n::t("player.comments.aiThinking"))
                    }))
                    .children((loading && items.is_empty()).then(|| {
                        div()
                            .text_xs()
                            .text_color(gpui_kit::rgba(0xffffff73))
                            .child(crate::i18n::t("common.loading"))
                    }))
                    .children(items.iter().map(|r| {
                        let digged = self
                            .comment_digg_override
                            .get(&r.reply_id)
                            .copied()
                            .unwrap_or(r.user_digg);
                        let digg_count = r.digg_count;
                        let rid = r.reply_id.clone();
                        let rcid = cid.clone();
                        let rname = r.user_name.clone();
                        let reply_mine =
                            !self.my_user_id().is_empty() && r.user_id == self.my_user_id();
                        h_flex()
                            .items_start()
                            .gap_2()
                            .child(self.render_avatar(&r.avatar, &r.user_name, 24.0))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(gpui_kit::rgba(0xffffff99))
                                            .child(SharedString::from(if r.user_name.is_empty() {
                                                crate::i18n::t("player.comments.anon")
                                            } else {
                                                r.user_name.clone()
                                            })),
                                    )
                                    .child({
                                        let original =
                                            crate::i18n::t("player.comments.originalComment");
                                        let name = if r.reply_to_name.is_empty() {
                                            original.as_str()
                                        } else {
                                            r.reply_to_name.as_str()
                                        };
                                        div()
                                            .text_xs()
                                            .text_color(gpui_kit::rgba(0xffffffd9))
                                            .child(SharedString::from(crate::i18n::tf(
                                                "player.comments.replyToSummary",
                                                &[("name", name), ("text", r.text.as_str())],
                                            )))
                                    })
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_3()
                                            .child(
                                                div()
                                                    .text_size(gpui_kit::px(10.0))
                                                    .text_color(gpui_kit::rgba(0xffffff73))
                                                    .child(crate::home::fmt::relative_time(
                                                        r.create_time,
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .id(ElementId::from(format!("reply-btn-{rid}")))
                                                    .text_size(gpui_kit::px(10.0))
                                                    .cursor_pointer()
                                                    .text_color(gpui_kit::rgba(0xffffff99))
                                                    .hover(|s| {
                                                        s.text_color(gpui_kit::rgb(0xffffff))
                                                    })
                                                    .on_click({
                                                        let rcid = rcid.clone();
                                                        let rid = rid.clone();
                                                        let rname = rname.clone();
                                                        cx.listener(move |this, _, _, cx| {
                                                            this.reply_to = Some((
                                                                rcid.clone(),
                                                                Some(rid.clone()),
                                                                rname.clone(),
                                                            ));
                                                            cx.notify();
                                                        })
                                                    })
                                                    .child(crate::i18n::t("player.comments.reply")),
                                            )
                                            .children(reply_mine.then(|| {
                                                div()
                                                    .id(ElementId::from(format!("reply-del-{rid}")))
                                                    .text_size(gpui_kit::px(10.0))
                                                    .cursor_pointer()
                                                    .text_color(gpui_kit::rgba(0xffffff73))
                                                    .hover(|s| {
                                                        s.text_color(gpui_kit::rgb(0xf87171))
                                                    })
                                                    .on_click({
                                                        let rid = rid.clone();
                                                        let rcid = rcid.clone();
                                                        cx.listener(move |this, _, _, cx| {
                                                            this.request_delete(
                                                                rid.clone(),
                                                                Some(rcid.clone()),
                                                                cx,
                                                            );
                                                        })
                                                    })
                                                    .child(crate::i18n::t("common.delete"))
                                            }))
                                            .child(
                                                // ♡ 回复点赞（乐观 ±1）
                                                div()
                                                    .id(ElementId::from(format!(
                                                        "reply-digg-{rid}"
                                                    )))
                                                    .flex()
                                                    .items_center()
                                                    .gap_0p5()
                                                    .cursor_pointer()
                                                    .text_color(if digged {
                                                        gpui_kit::rgb(0xef4444)
                                                    } else {
                                                        gpui_kit::rgba(0xffffff99)
                                                    })
                                                    .on_click({
                                                        let rid = rid.clone();
                                                        cx.listener(move |this, _, _, cx| {
                                                            this.toggle_reply_digg(rid.clone(), cx);
                                                        })
                                                    })
                                                    .child(
                                                        Icon::new(
                                                            gpui_kit::assets::IconName::Heart,
                                                        )
                                                        .size_3(),
                                                    )
                                                    .children((digg_count > 0).then(|| {
                                                        div().child(format!("{digg_count}"))
                                                    })),
                                            ),
                                    ),
                            )
                    }))
            }))
            .into_any_element()
    }
}

#[cfg(feature = "screenshot")]
impl super::HomeView {
    /// 取证辅助（screenshot feature 的 HONGGUO_SHOT_PANEL=comments）：
    /// 打开评论面板与表情选择器（无人值守抓网格 UI）。
    pub(crate) fn debug_open_comments(&mut self, cx: &mut Context<Self>) {
        self.comments_panel_open = true;
        self.emoji_picker_open = true;
        self.controls_until = Some(Instant::now() + Duration::from_secs(CONTROLS_HOLD_SECS));
        self.load_comments_first_page(cx);
        cx.notify();
    }

    /// 取证辅助（HONGGUO_SHOT_PANEL=mention）：打开评论面板，拉 @提及候选
    /// 并往输入框排入「@」——渲染期 set_value 后弹层即开（无人值守抓
    /// @候选弹层 UI）。
    pub(crate) fn debug_open_mention(&mut self, cx: &mut Context<Self>) {
        self.comments_panel_open = true;
        self.controls_until = Some(Instant::now() + Duration::from_secs(CONTROLS_HOLD_SECS));
        self.load_comments_first_page(cx);
        self.load_mention_users(cx);
        self.mention_insert = Some("@".to_string());
        cx.notify();
    }
}

/// 输入以 `@词` 结尾 = 提及候选弹层开着（Tauri 同款触发：打 @ 即弹；
/// `\u{FFFC}` 是提及段结尾占位符，含它说明已插入、不再触发）。手写匹配
/// （不引 regex）：`(^|\s)@[^\s@\u{FFFC}]*$` 同口径。
fn mention_query_open(text: &str) -> bool {
    let Some(at) = text.rfind('@') else {
        return false;
    };
    let after = &text[at + 1..];
    if after.contains('\u{FFFC}') {
        return false;
    }
    // @ 前必须是串首或空白
    if at > 0
        && !text[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
    {
        return false;
    }
    // @ 之后到串尾不许再有空白 / @ / 占位符
    !after
        .chars()
        .any(|c| c.is_whitespace() || c == '@' || c == '\u{FFFC}')
}

/// 提取 `@名字\u{FFFC}` 提及前缀（Tauri CommentText 的
/// `/^@([^\s\u{FFFC}]+)\u{FFFC}\s*/` 同口径），返回（名字, 余下正文）。
fn parse_mention_prefix(text: &str) -> Option<(String, &str)> {
    let rest = text.strip_prefix('@')?;
    let end = rest.find('\u{FFFC}')?;
    let name = &rest[..end];
    if name.is_empty() || name.chars().any(|c| c.is_whitespace() || c == '@') {
        return None;
    }
    let after = &rest[end + '\u{FFFC}'.len_utf8()..];
    Some((name.to_string(), after.trim_start()))
}

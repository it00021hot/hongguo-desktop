//! 评论输入区：回复目标、输入框、发送按钮与表情选择器。

use super::*;

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::Icon;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::*;

use crate::ui_button::button as Button;

impl HomeView {
    /// 底部发送框：@提及弹层、回复目标指示、输入框、表情与发送。
    pub(super) fn render_comment_composer(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let comment_input = self.comment_input.clone();
        // @提及候选弹层开关（Tauri mentionOpen 同款：输入以 `@词` 结尾即弹）
        let mention_open = comment_input
            .as_ref()
            .map(|i| super::mention_query_open(&i.read(cx).value()))
            .unwrap_or(false);
        let reply_target = self
            .reply_to
            .as_ref()
            .map(|(_, _, name)| name.clone())
            .unwrap_or_default();

        // 底部发送框：回复目标指示 + 输入 + 发送（Tauri border-white/10）
        v_flex()
            .px_4()
            .py_3()
            .gap_2()
            // relative：表情面板靠它定位（底边贴发送区顶边，见
            // render_emoji_picker）——发送区自己的行不会被面板盖住。
            .relative()
            .border_t_1()
            .border_color(gpui_kit::rgba(0xffffff1a))
            // 表情选择器：浮在发送区**上方**（列表区），底边对齐发送区
            // 顶边——附件预览、回复目标这些发送区自己的行始终可见
            // （Tauri 壳 EmojiPickerButton anchorTo="send-area" 同款）。
            .children(self.render_emoji_picker(cx))
            // @提及候选弹层（Tauri CommentPanel 同款：横排候选 +
            // AI 头像角标 + 「Ai」后缀；点选→延迟写入输入框）。
            // 表情选择器/候选都在输入行上方；无候选时给提示行。
            .children(mention_open.then(|| self.render_mention_popup(cx)))
            // 附图预览（官方同款：输入行上方的缩略图 + 删除钮）
            .children(self.comment_attachment.as_ref().and_then(|(item, _)| {
                self.emoticon_image(item).map(|image| {
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .relative()
                                .size_12()
                                .rounded(gpui_kit::px(6.0))
                                .border_1()
                                .border_color(gpui_kit::rgba(0xffffff1a))
                                .overflow_hidden()
                                .child(image),
                        )
                        .child(
                            BaseButton::new("comment-attachment-clear")
                                .accessibility_label(crate::i18n::t("player.emoticon.imageRemove"))
                                .size_6()
                                .cursor_pointer()
                                .justify_start()
                                .text_xs()
                                .text_color(gpui_kit::rgba(0xffffff99))
                                .hover(|s| s.text_color(gpui_kit::rgb(0xffffff)))
                                .focus_visible(|s| {
                                    s.shadow(crate::home::skin_focus::skin_focus_ring())
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.comment_attachment = None;
                                    cx.notify();
                                }))
                                .child(gpui_kit::assets::IconName::X),
                        )
                })
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
                        // 取消回复目标（base::Button 强制居中，这里改回
                        // 左对齐 + 默认行高，保持与原 div 同像素）
                        BaseButton::new("reply-cancel")
                            .accessibility_label(crate::i18n::t("common.cancel"))
                            .justify_start()
                            .line_height(gpui_kit::phi())
                            .text_xs()
                            .cursor_pointer()
                            .text_color(gpui_kit::rgba(0xffffff99))
                            .focus_visible(|s| s.shadow(crate::home::skin_focus::skin_focus_ring()))
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
                    .children(comment_input.as_ref().map(|input| {
                        // 输入框：固定深色皮肤（Tauri 评论面板
                        // 同款，不跟主题走）——`h-9 / rounded-md /
                        // bg-neutral-800/80 / px-3 / text-sm / 白字`，
                        // 没有描边；光标/占位符/选区由 [`skinned_input`]
                        // 在渲染当期改主题 token（否则亮色主题下是深灰
                        // 占位符 + 近黑光标压在深色面板上）。
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(gpui_kit::px(36.0))
                            .flex()
                            .items_center()
                            .rounded(gpui_kit::px(6.0))
                            .bg(gpui_kit::rgba(0x262626cc))
                            .px_3()
                            .child(super::super::controls::skinned_input(
                                Input::new(input)
                                    .token(|ctx, _, _| {
                                        crate::danmaku_emoji::render_emoji_token(ctx)
                                    })
                                    .appearance(false)
                                    .h_full()
                                    .flex_1()
                                    .text_sm()
                                    .text_color(gpui_kit::rgba(0xffffffff)),
                            ))
                    }))
                    // 表情按钮（Tauri EmojiSendBox 顺序：input → emoji →
                    // send；开时高亮，点击不夺输入框焦点，插入落真实光标处）
                    .child(
                        // 表情按钮（base::Button 强制居中，这里改回左对齐：
                        // 图标原本贴左；面板开合是开关语义）
                        BaseButton::new("emoji-btn")
                            .accessibility_label(crate::i18n::t("player.danmakuEmoji"))
                            .aria_toggled(if self.emoji_picker_open {
                                gpui_kit::Toggled::True
                            } else {
                                gpui_kit::Toggled::False
                            })
                            .justify_start()
                            .px_1()
                            .cursor_pointer()
                            .text_color(if self.emoji_picker_open {
                                gpui_kit::rgb(0xffffff)
                            } else {
                                gpui_kit::rgba(0xffffff99)
                            })
                            .hover(|s| s.text_color(gpui_kit::rgb(0xffffff)))
                            .focus_visible(|s| s.shadow(crate::home::skin_focus::skin_focus_ring()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.emoji_picker_open = !this.emoji_picker_open;
                                cx.notify();
                            }))
                            .child(
                                Icon::new(gpui_kit::assets::IconName::FaceGrinning)
                                    .size_4()
                                    .text_color(if self.emoji_picker_open {
                                        gpui_kit::rgb(0xffffff)
                                    } else {
                                        gpui_kit::rgba(0xffffff99)
                                    }),
                            ),
                    )
                    .child({
                        // 发送钮（Tauri `size="sm"` + `h-9 px-4` 同款）。
                        // 空文本/提交中不用 `disabled()`：禁用前景取主题
                        // muted，亮色主题下深色字压深底等于隐形；改用
                        // Tauri 的 `disabled:opacity-50` 视觉，点击由
                        // submit 内部护栏吞掉。
                        let empty = comment_input
                            .as_ref()
                            .is_none_or(|i| i.read(cx).value().trim().is_empty());
                        Button::new("comment-send")
                            .label(if self.reply_to.is_some() {
                                crate::i18n::t("player.comments.reply")
                            } else {
                                crate::i18n::t("player.interact.send")
                            })
                            .primary()
                            .h(gpui_kit::px(36.0))
                            .px(gpui_kit::px(16.0))
                            .rounded(gpui_kit::px(6.0))
                            .cursor_pointer()
                            .opacity(if empty || self.comment_sending {
                                0.5
                            } else {
                                1.0
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.reply_to.is_some() {
                                    this.submit_reply(cx);
                                } else {
                                    this.submit_comment(cx);
                                }
                            }))
                    }),
            )
            .into_any_element()
    }

    // 表情选择器：点外部收起的捕获层 + 上拉网格（Tauri EmojiPicker
    // 同款：8 列、每格一张表情图、连选不关面板、选完光标处插入）
    pub(super) fn render_emoji_backdrop(&self, cx: &Context<Self>) -> Option<gpui_kit::AnyElement> {
        self.emoji_picker_open.then(|| {
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
        })
    }

    pub(super) fn render_emoji_picker(&self, cx: &Context<Self>) -> Option<gpui_kit::AnyElement> {
        self.emoji_picker_open.then(|| {
            let comment_input = self.comment_input.clone();
            v_flex()
                .absolute()
                .left_3()
                // 底边 = 发送区顶边（bottom: 100%）：面板浮在评论列表上方，
                // 发送区的附件预览 / 回复目标 / 输入行都不在面板底下。
                .bottom(gpui_kit::relative(1.0))
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
                .children(self.render_emoticon_menu(cx))
                .child(self.render_emoticon_tabs(cx))
                .child(match self.emoticon_panel.tab {
                    super::emoticon::EmoticonTab::System => {
                        self.render_system_emoji_grid(comment_input.clone())
                    }
                    _ => self.render_remote_emoticon_grid(cx),
                })
                .into_any_element()
        })
    }

    /// 面板档位切换（官方三档：系统表情 / 第三方 GIF / 自定义表情）。
    fn render_emoticon_tabs(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let tab_row = |label: String, tab: super::emoticon::EmoticonTab, active: bool| {
            BaseButton::new(gpui_kit::ElementId::from(format!("emoticon-tab-{label}")))
                .accessibility_label(label.clone())
                .size_7()
                .rounded(gpui_kit::px(14.0))
                .cursor_pointer()
                .justify_start()
                .text_xs()
                .text_color(if active {
                    gpui_kit::rgb(0xffffff)
                } else {
                    gpui_kit::rgba(0xffffff99)
                })
                .hover(|s| s.text_color(gpui_kit::rgb(0xffffff)))
                .focus_visible(|s| s.shadow(crate::home::skin_focus::skin_focus_ring()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_emoticon_tab(tab, cx);
                }))
                .child(label)
        };
        h_flex()
            .items_center()
            .gap_1()
            .mb_2()
            .pb_2()
            .border_b_1()
            .border_color(gpui_kit::rgba(0xffffff14))
            .child(tab_row(
                crate::i18n::t("player.emoticon.system"),
                super::emoticon::EmoticonTab::System,
                self.emoticon_panel.tab == super::emoticon::EmoticonTab::System,
            ))
            .child(tab_row(
                crate::i18n::t("player.emoticon.thirdParty"),
                super::emoticon::EmoticonTab::ThirdParty,
                self.emoticon_panel.tab == super::emoticon::EmoticonTab::ThirdParty,
            ))
            .child(tab_row(
                crate::i18n::t("player.emoticon.custom"),
                super::emoticon::EmoticonTab::Own,
                self.emoticon_panel.tab == super::emoticon::EmoticonTab::Own,
            ))
            .into_any_element()
    }

    /// 系统表情网格（壳内内置 webp；插入 `[名字]` 文本码）。
    fn render_system_emoji_grid(
        &self,
        comment_input: Option<gpui_kit::Entity<gpui_kit::component::input::InputState>>,
    ) -> gpui_kit::AnyElement {
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
                // 每格一张表情图：库注入的 flex + 居中与原链
                // 完全一致（无需补偿）；格内无文字，行高也不必管。
                BaseButton::new(gpui_kit::ElementId::from(format!("emoji-{}", code)))
                    .accessibility_label(code.clone())
                    .size_8()
                    .rounded(gpui_kit::px(6.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(gpui_kit::rgba(0xffffff26)))
                    .focus_visible(|s| s.shadow(crate::home::skin_focus::skin_focus_ring()))
                    .on_click({
                        let input = comment_input.clone();
                        move |_, window, cx| {
                            if let Some(input) = &input {
                                // 光标处插入 `[名字]` 并附成原子内联
                                // token（焦点纪律：点格子不夺焦，选完
                                // 接着打字）；被拒时退回纯文本插入
                                input.update(cx, |state, cx| {
                                    let token = crate::danmaku_emoji::emoji_inline_token(&code);
                                    if state.replace_with_token(token, window, cx).is_err() {
                                        log::warn!("[Home] 表情 token 插入被拒，退回纯文本");
                                        state.insert(code.clone(), window, cx);
                                    }
                                });
                            }
                        }
                    })
                    .children(img.map(|img| {
                        gpui_kit::img(gpui_kit::ImageSource::Render(img))
                            .size_6()
                            .object_fit(gpui_kit::ObjectFit::Contain)
                    }))
            }))
            .into_any_element()
    }

    /// 远端表情网格（第三方 GIF / 自定义表情）：选中即作为附图。
    fn render_remote_emoticon_grid(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let scene = match self.emoticon_panel.tab {
            super::emoticon::EmoticonTab::ThirdParty => 2,
            _ => 4,
        };
        let items: Vec<hongguo_api::emoticon::Emoticon> = self.emoticon_panel.items().to_vec();
        if self.emoticon_panel.loading {
            return div()
                .id("emoticon-grid")
                .max_h(gpui_kit::px(176.0))
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(gpui_kit::rgba(0xffffff73))
                .child(crate::i18n::t("common.loading"))
                .into_any_element();
        }
        if items.is_empty() {
            return div()
                .id("emoticon-grid")
                .max_h(gpui_kit::px(176.0))
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(gpui_kit::rgba(0xffffff73))
                .child(crate::i18n::t("player.emoticon.empty"))
                .into_any_element();
        }
        div()
            .id("emoticon-grid")
            .max_h(gpui_kit::px(176.0))
            .overflow_y_scroll()
            .flex()
            .flex_wrap()
            .gap_1()
            .children(items.into_iter().enumerate().map(|(index, item)| {
                let key = item.id.clone().unwrap_or_else(|| format!("{index}"));
                let image = self.emoticon_image(&item);
                // 右键 = 桌面端等价于官方「长按」：弹「添加到表情 / 删除表情」。
                // 「我的表情」档里的是我自己的收藏（可删），第三方档不可删。
                let menu_item = item.clone();
                let can_remove = self.emoticon_panel.tab == super::emoticon::EmoticonTab::Own;
                BaseButton::new(gpui_kit::ElementId::from(format!("emoticon-{key}")))
                    .accessibility_label(item.image_name.clone().unwrap_or_default())
                    .size_8()
                    .rounded(gpui_kit::px(6.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(gpui_kit::rgba(0xffffff26)))
                    .focus_visible(|s| s.shadow(crate::home::skin_focus::skin_focus_ring()))
                    .on_mouse_down(
                        gpui_kit::MouseButton::Right,
                        cx.listener(move |this, _, _, cx| {
                            this.emoticon_menu = Some((menu_item.clone(), can_remove));
                            cx.notify();
                        }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.comment_attachment = Some((item.clone(), Some(scene)));
                        cx.notify();
                    }))
                    .children(image)
            }))
            .into_any_element()
    }

    /// 表情操作菜单条（官方同款：右键表情 → 「添加到表情」/「删除表情」）。
    fn render_emoticon_menu(&self, cx: &Context<Self>) -> Option<gpui_kit::AnyElement> {
        let (item, can_remove) = self.emoticon_menu.clone()?;
        let remove_item = item.clone();
        Some(
            h_flex()
                .items_center()
                .gap_2()
                .mb_2()
                .pb_2()
                .border_b_1()
                .border_color(gpui_kit::rgba(0xffffff14))
                .child(
                    BaseButton::new("emoticon-add-to-mine")
                        .accessibility_label(crate::i18n::t("player.emoticon.addToMine"))
                        .justify_start()
                        .text_xs()
                        .cursor_pointer()
                        .text_color(gpui_kit::rgba(0xffffff99))
                        .hover(|s| s.text_color(gpui_kit::rgb(0xffffff)))
                        .focus_visible(|s| s.shadow(crate::home::skin_focus::skin_focus_ring()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.emoticon_menu = None;
                            this.collect_emoticon(&item, cx);
                        }))
                        .child(crate::i18n::t("player.emoticon.addToMine")),
                )
                .children(can_remove.then(|| {
                    BaseButton::new("emoticon-remove-from-mine")
                        .accessibility_label(crate::i18n::t("player.emoticon.removeFromMine"))
                        .justify_start()
                        .text_xs()
                        .cursor_pointer()
                        .text_color(gpui_kit::rgba(0xffffff99))
                        .hover(|s| s.text_color(gpui_kit::rgb(0xffffff)))
                        .focus_visible(|s| s.shadow(crate::home::skin_focus::skin_focus_ring()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.emoticon_menu = None;
                            this.emoticon_delete_target = Some(remove_item.clone());
                            cx.notify();
                        }))
                        .child(crate::i18n::t("player.emoticon.removeFromMine"))
                }))
                .into_any_element(),
        )
    }
}

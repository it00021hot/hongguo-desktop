//! 评论输入区：回复目标、输入框、发送按钮与表情选择器。

use super::*;

use gpui_kit::component::Icon;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::*;

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
            .border_t_1()
            .border_color(gpui_kit::rgba(0xffffff1a))
            // @提及候选弹层（Tauri CommentPanel 同款：横排候选 +
            // AI 头像角标 + 「Ai」后缀；点选→延迟写入输入框）。
            // 表情选择器/候选都在输入行上方；无候选时给提示行。
            .children(mention_open.then(|| self.render_mention_popup(cx)))
            // 表情/提及预览（GPUI 的 Input 是纯文本编辑器，插进去的 `[名字]`
            // 只能是代码；预览行按弹幕层那套分段渲染出表情图与蓝色 @名字，
            // 发出去之前先看见最终长什么样——Tauri 那边是 contentEditable
            // 内联渲染，这里用预览补齐同一件事）。
            .children(
                self.render_rich_preview(
                    &comment_input
                        .as_ref()
                        .map(|i| i.read(cx).value().to_string())
                        .unwrap_or_default(),
                ),
            )
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
                    .children(comment_input.as_ref().map(|input| {
                        // 输入框：hgplayer 固定深色皮肤（Tauri 评论面板
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
        })
    }
}

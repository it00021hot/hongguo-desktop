//! 剧评发评框（Tauri ReviewComposer 同款：我的评分五星 + 表情 + 文本 +
//! 「发表剧评」）。从 `view.rs` 拆出：文件规模拆分 + 评分星/表情的键盘可达化
//! （base Button 承载焦点句柄与 Enter/Space 激活）。

use gpui_kit::base::Button as BaseButton;
use gpui_kit::component::Disableable;
use gpui_kit::component::Icon;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::{ElementId, Role, Toggled, div};

use crate::detail::header::star_color;
use crate::ui_button::button as Button;

impl super::super::DetailView {
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
        // 评分星：外层 radiogroup（Tauri ReviewComposer 同款 role=radiogroup
        // 只包 5 颗星，「我的评分」标签留在组外），每颗星是 base Button
        // （键盘可达）+ RadioButton 角色；hover 预览保留。
        let mut star_group = h_flex()
            .id("review-stars")
            .role(Role::RadioGroup)
            .aria_label(crate::i18n::t("detail.myRating"))
            .gap_2();
        for s in 1..=5u8 {
            star_group = star_group.child(
                BaseButton::new(ElementId::from(format!("review-star-{s}")))
                    .role(Role::RadioButton)
                    .aria_toggled(if self.review_stars == s {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .accessibility_label(crate::i18n::tf(
                        "detail.rateStar",
                        &[("n", &s.to_string())],
                    ))
                    .p_0p5()
                    .cursor_pointer()
                    .focus_visible(super::view::focus_ring_style(theme))
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
        let mut stars = h_flex().items_center().gap_2().child(
            div()
                .text_sm()
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .child(crate::i18n::t("detail.myRating")),
        );
        stars = stars.child(star_group);
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
                        BaseButton::new("review-emoji-btn")
                            .aria_toggled(if self.review_emoji_open {
                                Toggled::True
                            } else {
                                Toggled::False
                            })
                            .accessibility_label(crate::i18n::t("player.danmakuEmoji"))
                            .px_1()
                            .cursor_pointer()
                            .text_color(if self.review_emoji_open {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .hover(|s| s.text_color(theme.foreground))
                            .focus_visible(super::view::focus_ring_style(theme))
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
                            .when(!can_send, |b| b.cursor_default())
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
                        BaseButton::new(ElementId::from(format!("review-emoji-{code}")))
                            .accessibility_label(code.clone())
                            .size_7()
                            .rounded(gpui_kit::px(6.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.secondary_hover))
                            // 焦点环只在键盘聚焦时出现（focus_visible），静止时
                            // 无视觉负担；但少了它，键盘用户 Tab 进这 56 格就
                            // 不知道停在哪一格。
                            .focus_visible(super::view::focus_ring_style(theme))
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
}

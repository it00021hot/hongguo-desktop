use super::{
    ButtonVariant,
    controls::surface,
    motion::{FAST_MS, PRESS_SCALE},
};
use nsh_base::{PopoverEvent, theme};
use nsh_core::{element::ImageSrc, prelude::*};

pub(crate) enum InlineEmojiPart {
    Text(String),
    Image {
        code: &'static str,
        uri: &'static str,
    },
    Mention(String),
}

#[derive(Clone, Copy)]
pub(crate) enum EmojiTextStyle {
    Body,
    Comment,
    Reply,
    Review,
    ReviewReply,
    Caption,
    Danmaku(f32),
}

pub(crate) fn emoji_trigger(
    glyph: super::Icon,
    label: impl IntoReactive<SharedString>,
    skin: EmojiTextStyle,
) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    surface(ButtonVariant::Quiet)
        .accessibility_label(move || Some(label.get()))
        .min_h_0()
        .p_1()
        .size(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .text_color(move || match skin {
            EmojiTextStyle::Body | EmojiTextStyle::Review | EmojiTextStyle::ReviewReply => {
                theme().foreground
            }
            EmojiTextStyle::Comment | EmojiTextStyle::Reply => {
                theme().color("video-picker-text").expect("picker text")
            }
            EmojiTextStyle::Caption | EmojiTextStyle::Danmaku(_) => {
                theme().color("video-controls").expect("controls")
            }
        })
        .child(super::icon(glyph).size_4())
}

pub(crate) fn emoji_paragraph(parts: Vec<InlineEmojiPart>, style: EmojiTextStyle) -> Text {
    let font_size = match style {
        EmojiTextStyle::Body | EmojiTextStyle::Comment | EmojiTextStyle::Review => 14.0,
        EmojiTextStyle::Reply | EmojiTextStyle::ReviewReply => 12.0,
        EmojiTextStyle::Caption => 16.0,
        EmojiTextStyle::Danmaku(size) => size,
    };
    let content = parts.into_iter().map(|part| match part {
        InlineEmojiPart::Text(value) => text(value)
            .font_weight(if matches!(style, EmojiTextStyle::Danmaku(_)) {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            })
            .into_element(),
        InlineEmojiPart::Mention(name) if matches!(style, EmojiTextStyle::Danmaku(_)) => {
            text(format!("@{name}")).into_element()
        }
        InlineEmojiPart::Mention(name) => text(format!("@{name}"))
            .mr_1()
            .text_color(move || theme().color("video-mention").expect("mention token"))
            .into_element(),
        InlineEmojiPart::Image { code, uri } => image(ImageSrc::from(uri))
            .label(code)
            .size(move || {
                px(font_size
                    * if matches!(style, EmojiTextStyle::Danmaku(_)) {
                        1.3
                    } else {
                        1.15
                    }
                    * font_scale())
            })
            .align_baseline()
            .translate_y(move || {
                px(font_size
                    * if matches!(style, EmojiTextStyle::Danmaku(_)) {
                        0.28
                    } else {
                        0.18
                    }
                    * font_scale())
            })
            .object_fit(nsh_core::ResizeMode::Contain)
            .into_element(),
    });
    let paragraph = text("").children(content);
    match style {
        EmojiTextStyle::Body => paragraph
            .text_sm()
            .line_height(nsh_core::LineHeight::Multiple(1.375)),
        EmojiTextStyle::Comment | EmojiTextStyle::Reply => paragraph
            .text_size(move || px(if is_desktop() { font_size } else { 16.0 }))
            .font_weight(FontWeight::NORMAL)
            .line_height(move || {
                nsh_core::LineHeight::Multiple(if is_desktop() { 1.375 } else { 1.5 })
            })
            .text_color(move || {
                if is_desktop() {
                    theme().color("video-picker-text").expect("picker text")
                } else {
                    theme().foreground
                }
            }),
        EmojiTextStyle::Review | EmojiTextStyle::ReviewReply => paragraph
            .text_size(move || px(if is_desktop() { font_size } else { 16.0 }))
            .font_weight(FontWeight::NORMAL)
            .line_height(move || {
                nsh_core::LineHeight::Multiple(if !is_desktop() {
                    1.5
                } else if matches!(style, EmojiTextStyle::Review) {
                    1.625
                } else {
                    1.375
                })
            }),
        EmojiTextStyle::Caption => paragraph
            .text_base()
            .number_of_lines(1)
            .text_color(move || theme().color("video-caption").expect("caption token")),
        EmojiTextStyle::Danmaku(size) => paragraph
            .text_size(px(size))
            .font_weight(FontWeight::SEMIBOLD)
            .number_of_lines(1)
            .text_color(move || theme().color("video-caption").expect("caption token"))
            .text_shadow([nsh_core::TextShadow {
                color: {
                    let color = theme()
                        .color("video-surface")
                        .expect("surface")
                        .opacity(0.9);
                    if dark() {
                        color.dark.into()
                    } else {
                        color.light.into()
                    }
                },
                offset_x: px(0.0),
                offset_y: px(1.0),
                radius: px(3.0),
            }]),
    }
}

pub(crate) fn emoji_picker(
    open: Signal<bool>,
    trigger: nsh_base::ButtonElement,
    content: impl IntoElement,
    closed: impl Fn() + 'static,
) -> nsh_base::PopoverElement {
    nsh_base::popover()
        .open(move || Some(open.get()))
        .on_open_change(move |event: &PopoverEvent| {
            if let PopoverEvent::OpenChange(value, _) = event {
                open.set(*value);
            }
        })
        .on_close(closed)
        .placement(Placement::Top)
        .align(Align::End)
        .offset(px(8.0))
        .margin(px(8.0))
        .trigger(trigger)
        .popup_style(|style| {
            style
                .p_2()
                .rounded_xl()
                .border_1()
                .border_color(move || {
                    theme()
                        .color("video-controls")
                        .expect("video controls")
                        .opacity(0.1)
                })
                .bg(move || {
                    theme()
                        .color("video-surface")
                        .expect("video surface")
                        .opacity(0.9)
                })
                .text_color(move || theme().color("video-controls").expect("video controls"))
                .shadow_lg()
                .origin_bottom_right()
                .transition()
                .duration(move || if reduce_motion() { 0.0 } else { FAST_MS })
                .ease_out()
        })
        .popup_starting_style(|style| {
            style
                .opacity(0.0)
                .scale(move || if reduce_motion() { 1.0 } else { PRESS_SCALE })
        })
        .popup_ending_style(|style| {
            style
                .opacity(0.0)
                .scale(move || if reduce_motion() { 1.0 } else { PRESS_SCALE })
        })
        .child(content)
}

pub(crate) fn emoji_system_cell(code: &'static str, uri: &'static str) -> nsh_base::ButtonElement {
    surface(ButtonVariant::Quiet)
        .accessibility_label(code)
        .min_h_0()
        .px_1()
        .py_1()
        .size(move || px(if is_desktop() { 34.25 } else { 48.0 }))
        .rounded_md()
        .hover(|style| {
            style.bg(move || {
                theme()
                    .color("video-controls")
                    .expect("video controls")
                    .opacity(0.15)
            })
        })
        .child(
            image(ImageSrc::from(uri))
                .size_6()
                .object_fit(nsh_core::ResizeMode::Contain),
        )
}

pub(crate) fn emoji_panel() -> View {
    view().w(move || px(panel_width())).gap_2()
}

pub(crate) fn emoji_comment_image(src: impl IntoReactive<ImageSrc>) -> impl IntoElement {
    image(src)
        .label(move || crate::i18n::tr("player.emoticon.image"))
        .max_h_40()
        .rounded_md()
        .border_1()
        .border_color(move || {
            theme()
                .color("video-controls")
                .expect("controls")
                .opacity(0.1)
        })
        .object_fit(nsh_core::ResizeMode::Cover)
}

fn panel_width() -> f32 {
    (if is_desktop() { 288.0_f32 } else { 308.0 }).min(window().width.0 - 32.0)
}

pub(crate) fn emoji_metrics(small: bool) -> (usize, f32, f32) {
    let gap = if small { 2.0 } else { 4.0 };
    let columns = if is_desktop() {
        if small { 8 } else { 6 }
    } else {
        (((panel_width() + gap) / (48.0 + gap)).floor() as usize).clamp(1, 6)
    };
    (
        columns,
        (panel_width() - gap * (columns - 1) as f32) / columns as f32,
        gap,
    )
}

pub(crate) fn emoji_tab(
    label: impl IntoReactive<SharedString>,
    glyph: Option<super::Icon>,
) -> nsh_base::ButtonElement {
    let label = label.into_reactive();
    let named = label.clone();
    surface(ButtonVariant::Quiet)
        .accessibility_label(move || named.get())
        .min_h_0()
        .size(move || px(if is_desktop() { 28.0 } else { 48.0 }))
        .p_1()
        .rounded_full()
        .text_color(move || {
            theme()
                .color("video-controls")
                .expect("controls")
                .opacity(0.75)
        })
        .hover(|style| {
            style.bg(move || {
                theme()
                    .color("video-controls")
                    .expect("controls")
                    .opacity(0.1)
            })
        })
        .selected_style(|style| {
            style.border(px(0.0)).bg(move || {
                theme()
                    .color("video-controls")
                    .expect("controls")
                    .opacity(0.15)
            })
        })
        .child(match glyph {
            Some(icon) => super::icon(icon).size_4().into_element(),
            None => text("GIF").text_xs().into_element(),
        })
}

pub(crate) fn emoji_search() -> nsh_base::Input {
    super::field(move || crate::i18n::tr("player.emoticon.searchPlaceholder"))
        .min_h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .h(move || px(if is_desktop() { 32.0 } else { 48.0 }))
        .text_xs()
        .border(px(0.0))
        .bg(move || theme().color("video-picker-cell").expect("picker cell"))
        .text_color(move || theme().color("video-controls").expect("controls"))
}

pub(crate) fn emoji_hint(value: impl IntoReactive<SharedString>) -> Text {
    text(value)
        .text_xs()
        .text_color(move || theme().color("video-picker-muted").expect("picker muted"))
}

pub(crate) fn emoji_image_cell(
    src: impl IntoReactive<String>,
    label: impl IntoReactive<SharedString>,
    small: bool,
) -> nsh_base::ButtonElement {
    let src = src.into_reactive();
    let label = label.into_reactive();
    surface(ButtonVariant::Quiet)
        .accessibility_label(move || Some(label.get()))
        .min_h_0()
        .p_1()
        .rounded_md()
        .size(move || px(emoji_metrics(small).1))
        .hover(|style| {
            style.bg(move || {
                theme()
                    .color("video-controls")
                    .expect("controls")
                    .opacity(0.15)
            })
        })
        .child(
            image(move || ImageSrc::from(src.get()))
                .size_full()
                .object_fit(nsh_core::ResizeMode::Contain),
        )
}

pub(crate) fn emoji_context(
    trigger: impl IntoElement,
    remove: impl Fn() -> bool + 'static,
    action: impl Fn() + 'static,
) -> nsh_base::ContextMenuElement {
    let remove = stored(Box::new(remove) as Box<dyn Fn() -> bool>);
    nsh_base::context_menu()
        .trigger(trigger)
        .popup_style(|style| {
            style
                .p_1()
                .rounded_lg()
                .border_1()
                .border_color(move || {
                    theme()
                        .color("video-controls")
                        .expect("controls")
                        .opacity(0.1)
                })
                .bg(move || {
                    theme()
                        .color("video-surface")
                        .expect("surface")
                        .opacity(0.9)
                })
                .shadow_lg()
        })
        .child(
            nsh_base::menu_item()
                .on_select(action)
                .px_3()
                .py_2()
                .rounded_md()
                .hover(|style| {
                    style.bg(move || {
                        theme()
                            .color("video-controls")
                            .expect("controls")
                            .opacity(0.1)
                    })
                })
                .child(
                    text(move || {
                        crate::i18n::tr(if remove.with(|remove| remove()) {
                            "player.emoticon.removeFromMine"
                        } else {
                            "player.emoticon.addToMine"
                        })
                    })
                    .text_xs()
                    .text_color(move || theme().color("video-controls").expect("controls")),
                ),
        )
}

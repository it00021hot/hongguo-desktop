use super::{Attachment, Editor};
use crate::{i18n::tr, ui};
use nsh_core::{element::ImageSrc, prelude::*};

pub(crate) struct SendEvents {
    pub send: Box<dyn Fn()>,
    pub pick_image: Box<dyn Fn()>,
}

pub(crate) struct SendOptions {
    pub allowed: Memo<bool>,
    pub review: bool,
    pub reply: bool,
    pub previous: Box<dyn Fn()>,
    pub next: ElementRef,
}

pub(crate) fn sendbox(
    editor: Editor,
    pending: Signal<bool>,
    _logged_in: Memo<bool>,
    attachment: Option<Signal<Option<Attachment>>>,
    picker: impl IntoElement,
    events: SendEvents,
    options: SendOptions,
) -> impl IntoElement {
    let events = stored(std::rc::Rc::new(events));
    let previous = stored(std::rc::Rc::new(options.previous));
    let next = options.next;
    let review = options.review;
    let reply = options.reply;
    let allowed = options.allowed;
    let can_send = computed(move || {
        allowed.get()
            && !pending.get()
            && (!editor.text.get().trim().is_empty()
                || attachment.is_some_and(|image| image.with(Option::is_some)))
    });
    view()
        .gap_2()
        .child(when(
            move || attachment.is_some_and(|image| image.with(Option::is_some)),
            move || {
                view()
                    .relative()
                    .size_16()
                    .rounded_md()
                    .border_1()
                    .border_color(move || nsh_base::theme().border)
                    .child(
                        image(move || {
                            ImageSrc::from(
                                attachment
                                    .and_then(|image| image.get())
                                    .map_or_else(String::new, |image| image.preview()),
                            )
                        })
                        .label(move || tr("player.emoticon.image"))
                        .size_full()
                        .object_fit(nsh_core::ResizeMode::Contain),
                    )
                    .child(
                        ui::player_icon_button(ui::Icon::Close, move || {
                            tr("player.emoticon.imageRemove")
                        })
                        .absolute()
                        .top(px(-6.0))
                        .right(px(-6.0))
                        .size_5()
                        .p_0()
                        .min_h_0()
                        .disabled(pending)
                        .on_press(move || {
                            if let Some(image) = attachment {
                                image.set(None);
                            }
                        }),
                    )
            },
        ))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(if review && !reply { 12.0 } else { 8.0 }))
                .child(when(
                    move || attachment.is_some(),
                    move || {
                        ui::emoji_trigger(
                            ui::Icon::ImagePlus,
                            move || tr("player.emoticon.image"),
                            ui::EmojiTextStyle::Comment,
                        )
                        .disabled(pending)
                        .on_press(move || (events.get().pick_image)())
                    },
                ))
                .child(
                    view()
                        .flex_1()
                        .min_w_0()
                        .child(editor.field(super::FieldOptions {
                            placeholder: computed(move || {
                                tr(if review && !reply {
                                    "detail.commentPlaceholder"
                                } else {
                                    "player.comments.placeholder"
                                })
                            }),
                            disabled: computed(move || pending.get()),
                            multiline: review && !reply,
                            skin: match (review, reply) {
                                (true, false) => super::FieldSkin::Review,
                                (true, true) => super::FieldSkin::ReviewReply,
                                (false, false) => super::FieldSkin::Comment,
                                (false, true) => super::FieldSkin::Reply,
                            },
                            submit: Box::new(move || {
                                if can_send.get_untracked() {
                                    (events.get().send)();
                                }
                            }),
                            on_tab: Box::new(move |backward| {
                                if backward {
                                    (previous.get())();
                                } else {
                                    next.focus();
                                }
                            }),
                        })),
                )
                .child(picker)
                .child(
                    ui::button(
                        move || {
                            tr(if pending.get() {
                                "common.loading"
                            } else if review && !reply {
                                "detail.reviewSend"
                            } else {
                                "player.interact.send"
                            })
                        },
                        ui::ButtonVariant::Primary,
                    )
                    .min_h_0()
                    .h(move || {
                        px(if is_desktop() {
                            if reply { 32.0 } else { 36.0 }
                        } else {
                            48.0
                        })
                    })
                    .px(px(if reply { 12.0 } else { 16.0 }))
                    .py_0()
                    .disabled(move || !can_send.get())
                    .on_press(move || (events.get().send)()),
                ),
        )
}

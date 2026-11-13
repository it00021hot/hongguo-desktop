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
    pub previous: Box<dyn Fn()>,
    pub next: ElementRef,
}

pub(crate) fn sendbox(
    editor: Editor,
    pending: Signal<bool>,
    logged_in: Memo<bool>,
    attachment: Option<Signal<Option<Attachment>>>,
    picker: impl IntoElement,
    events: SendEvents,
    options: SendOptions,
) -> impl IntoElement {
    let events = stored(events);
    let previous = stored(options.previous);
    let next = options.next;
    let review = options.review;
    let allowed = options.allowed;
    let can_send = computed(move || {
        logged_in.get()
            && allowed.get()
            && !pending.get()
            && (!editor.text.get().trim().is_empty()
                || attachment.is_some_and(|image| image.with(Option::is_some)))
    });
    view()
        .gap_2()
        .child(editor.field(super::FieldOptions {
            placeholder: computed(move || {
                tr(if review {
                    "detail.commentPlaceholder"
                } else {
                    "player.comments.placeholder"
                })
            }),
            disabled: computed(move || !logged_in.get() || pending.get()),
            multiline: review,
            skin: if review {
                super::FieldSkin::Review
            } else {
                super::FieldSkin::Body
            },
            submit: Box::new(move || {
                if can_send.get_untracked() {
                    events.with(|events| (events.send)());
                }
            }),
            on_tab: Box::new(move |backward| {
                if backward {
                    previous.with(|focus| focus());
                } else {
                    next.focus();
                }
            }),
        }))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .child(picker)
                .child(when(
                    move || attachment.is_some(),
                    move || {
                        ui::emoji_trigger(
                            ui::Icon::ImagePlus,
                            move || tr("player.emoticon.image"),
                            ui::EmojiTextStyle::Body,
                        )
                        .disabled(move || !logged_in.get() || pending.get())
                        .on_press(move || {
                            if pending.get_untracked() {
                                return;
                            }
                            events.with(|events| (events.pick_image)());
                        })
                    },
                ))
                .child(
                    ui::button(
                        move || {
                            tr(if pending.get() {
                                "common.loading"
                            } else if review {
                                "detail.reviewSend"
                            } else {
                                "player.interact.send"
                            })
                        },
                        ui::ButtonVariant::Primary,
                    )
                    .disabled(move || !can_send.get())
                    .on_press(move || events.with(|events| (events.send)())),
                ),
        )
        .child(when(
            move || attachment.is_some_and(|image| image.with(Option::is_some)),
            move || {
                view()
                    .flex_row()
                    .items_start()
                    .gap_1()
                    .child(
                        image(move || {
                            ImageSrc::from(
                                attachment
                                    .and_then(|image| image.get())
                                    .map_or_else(String::new, |image| image.preview()),
                            )
                        })
                        .label(move || tr("player.emoticon.image"))
                        .size_24()
                        .object_fit(nsh_core::ResizeMode::Contain),
                    )
                    .child(
                        ui::button(
                            move || tr("player.emoticon.imageRemove"),
                            ui::ButtonVariant::Quiet,
                        )
                        .disabled(pending)
                        .on_press(move || {
                            if let Some(image) = attachment {
                                image.set(None);
                            }
                        }),
                    )
            },
        ))
}

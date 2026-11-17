pub(crate) mod presentation;
use hongguo_service::emoticon_service as api;
mod attachment;
mod comment_image;
mod controller;
mod editor;
mod model;
mod own_edits;
pub(crate) use own_edits::CollectionMemory;
mod panel;
mod remote_model;
mod sendbox;
pub(crate) use attachment::pick_local;
pub(crate) use comment_image::comment_image;
pub(crate) use controller::picker;
pub(crate) use editor::{Editor, FieldOptions, FieldSkin};
pub(crate) use hongguo_service::emoticon_service::attachment::{Attachment, Scene};

pub(crate) use model::{SYSTEM, Segment, parse};
use nsh_core::prelude::*;
pub(crate) use sendbox::{SendEvents, SendOptions, sendbox};

pub(crate) fn render(
    value: impl IntoReactive<String>,
    style: crate::features::emoji::presentation::EmojiTextStyle,
) -> impl IntoElement {
    let value = value.into_reactive();
    dynamic(move || {
        crate::features::emoji::presentation::emoji_paragraph(
            parse(&value.get())
                .into_iter()
                .map(|part| match part {
                    Segment::Text(value) => {
                        crate::features::emoji::presentation::InlineEmojiPart::Text(value)
                    }
                    Segment::Emoji(emoji) => {
                        crate::features::emoji::presentation::InlineEmojiPart::Image {
                            code: emoji.code,
                            uri: emoji.uri,
                        }
                    }
                    Segment::Mention { name, .. } => {
                        crate::features::emoji::presentation::InlineEmojiPart::Mention(name)
                    }
                })
                .collect(),
            style,
        )
    })
}

pub(crate) fn system_picker(editor: Editor, trigger: ElementRef) -> impl IntoElement {
    system_picker_skin(
        editor,
        trigger,
        crate::features::emoji::presentation::EmojiTextStyle::Caption,
    )
}

pub(crate) fn comment_picker(editor: Editor, trigger: ElementRef) -> impl IntoElement {
    system_picker_skin(
        editor,
        trigger,
        crate::features::emoji::presentation::EmojiTextStyle::Body,
    )
}

pub(crate) fn floating_picker(editor: Editor, trigger: ElementRef) -> impl IntoElement {
    system_picker_skin(
        editor,
        trigger,
        crate::features::emoji::presentation::EmojiTextStyle::FloatingCaption,
    )
}

fn system_picker_skin(
    editor: Editor,
    trigger: ElementRef,
    skin: crate::features::emoji::presentation::EmojiTextStyle,
) -> impl IntoElement {
    let open = signal(false);
    let notice = signal(SharedString::default());
    let feed = signal(remote_model::Feed::default());
    crate::features::emoji::presentation::emoji_picker(
        open,
        crate::features::emoji::presentation::emoji_trigger(
            crate::ui::Icon::Smile,
            move || crate::i18n::tr("player.danmakuEmoji"),
            skin,
        )
        .element_ref(&trigger)
        .selected(open),
        panel::panel(
            panel::State {
                feed,
                notice,
                busy: computed(|| false),
                failed: computed(|| false),
            },
            false,
            move |intent| {
                if let panel::Intent::Insert(code) = intent
                    && let Err(error) = editor.insert(code)
                {
                    notice.set(error);
                }
            },
        ),
        move || editor.reference.focus(),
    )
}

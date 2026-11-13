mod api;
mod attachment;
mod comment_image;
mod controller;
mod editor;
mod fixture;
mod model;
mod own_edits;
pub(crate) use own_edits::CollectionMemory;
mod panel;
mod remote_model;
mod sendbox;
#[cfg(test)]
mod tests;
pub(crate) use attachment::pick_local;
pub(crate) use attachment::{Attachment, Scene};
pub(crate) use comment_image::comment_image;
pub(crate) use controller::picker;
pub(crate) use editor::{Editor, FieldOptions, FieldSkin};
pub(crate) use fixture::fixture;
pub(crate) use model::{SYSTEM, Segment, parse};
use nsh_core::prelude::*;
pub(crate) use sendbox::{SendEvents, SendOptions, sendbox};

pub(crate) fn render(
    value: impl IntoReactive<String>,
    style: crate::ui::EmojiTextStyle,
) -> impl IntoElement {
    let value = value.into_reactive();
    dynamic(move || {
        crate::ui::emoji_paragraph(
            parse(&value.get())
                .into_iter()
                .map(|part| match part {
                    Segment::Text(value) => crate::ui::InlineEmojiPart::Text(value),
                    Segment::Emoji(emoji) => crate::ui::InlineEmojiPart::Image {
                        code: emoji.code,
                        uri: emoji.uri,
                    },
                    Segment::Mention { name, .. } => crate::ui::InlineEmojiPart::Mention(name),
                })
                .collect(),
            style,
        )
    })
}

pub(crate) fn system_picker(editor: Editor, trigger: ElementRef) -> impl IntoElement {
    system_picker_skin(editor, trigger, crate::ui::EmojiTextStyle::Caption)
}

pub(crate) fn comment_picker(editor: Editor, trigger: ElementRef) -> impl IntoElement {
    system_picker_skin(editor, trigger, crate::ui::EmojiTextStyle::Body)
}

fn system_picker_skin(
    editor: Editor,
    trigger: ElementRef,
    skin: crate::ui::EmojiTextStyle,
) -> impl IntoElement {
    let open = signal(false);
    let notice = signal(SharedString::default());
    let feed = signal(remote_model::Feed::default());
    crate::ui::emoji_picker(
        open,
        crate::ui::emoji_trigger(
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

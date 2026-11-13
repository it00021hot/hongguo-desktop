use crate::{i18n::tr, ui};
use nsh_core::prelude::*;

pub(crate) struct ComposerEvents {
    pub(crate) send: Box<dyn Fn()>,
    pub(crate) emoji: Option<AnyElement>,
    pub(crate) previous: Memo<ElementRef>,
    pub(crate) next: ElementRef,
}

pub(crate) fn composer(
    draft: crate::emoji::Editor,
    disabled: impl IntoReactive<bool>,
    events: ComposerEvents,
) -> View {
    let disabled = disabled.into_reactive();
    let disabled = computed(move || disabled.get());
    let emoji = events.emoji;
    let previous = events.previous;
    let next = events.next;
    let events = stored(events.send);
    let variant = computed(|| {
        if super::landscape::active() {
            ui::PlayerComposerVariant::Landscape
        } else {
            ui::PlayerComposerVariant::Toolbar
        }
    });
    ui::player_send_surface(variant)
        .flex_shrink(1.0)
        .min_w(px(0.0))
        .child(
            draft
                .field(crate::emoji::FieldOptions {
                    placeholder: computed(move || tr("player.interact.danmakuPlaceholder")),
                    disabled,
                    multiline: false,
                    skin: crate::emoji::FieldSkin::Video,
                    submit: Box::new(move || events.with(|send| send())),
                    on_tab: Box::new(move |backward| {
                        if backward {
                            previous.get_untracked().focus();
                        } else {
                            next.focus();
                        }
                    }),
                })
                .flex_1(),
        )
        .child(emoji.unwrap_or_else(|| {
            ui::player_icon_button(ui::Icon::Smile, move || tr("player.danmakuEmoji"))
                .size(px(24.0))
                .disabled(true)
                .into_element()
        }))
        .child(
            ui::player_send_action(move || tr("player.interact.send"), variant)
                .disabled(move || disabled.get() || draft.text.get().trim().is_empty())
                .on_press(move || events.with(|send| send())),
        )
}

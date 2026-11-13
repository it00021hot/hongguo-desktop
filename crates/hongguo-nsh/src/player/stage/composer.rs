use crate::{i18n::tr, ui};
use nsh_core::prelude::*;

pub(crate) struct ComposerEvents {
    pub(crate) send: Box<dyn Fn()>,
    pub(crate) emoji: Option<Box<dyn Fn() -> AnyElement>>,
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
    let events = stored(std::rc::Rc::new(events));
    let variant = computed(|| {
        if super::landscape::active() {
            ui::PlayerComposerVariant::Landscape
        } else {
            ui::PlayerComposerVariant::Toolbar
        }
    });
    let geometry = use_context::<ui::PlayerGeometry>();
    let collapsed = computed(move || {
        geometry
            .is_some_and(|geometry| super::desktop_composer_collapsed(geometry.viewport.get().0))
    });
    let open = signal(false);
    effect(move || {
        if !collapsed.get() {
            open.set(false);
        }
    });
    view()
        .flex_shrink_0()
        .child(when(
            move || !collapsed.get(),
            move || input(draft, disabled, events, variant, false),
        ))
        .child(when(collapsed, move || {
            ui::player_send_popover(open, move || input(draft, disabled, events, variant, true))
        }))
}

fn input(
    draft: crate::emoji::Editor,
    disabled: Memo<bool>,
    events: Stored<std::rc::Rc<ComposerEvents>>,
    variant: Memo<ui::PlayerComposerVariant>,
    floating: bool,
) -> View {
    let focused = stored(false);
    let content = ui::player_send_surface(variant)
        .flex_shrink_0()
        .style_when(floating, |style| style.w(px(320.0)).min_w(px(320.0)).ml_0())
        .on_layout(move |_: &Bounds<Points>| {
            if floating && !focused.replace(true) {
                draft.reference.focus();
            }
        })
        .child(
            draft
                .field(crate::emoji::FieldOptions {
                    placeholder: computed(move || tr("player.interact.danmakuPlaceholder")),
                    disabled,
                    multiline: false,
                    skin: crate::emoji::FieldSkin::Video,
                    submit: Box::new(move || (events.get().send)()),
                    on_tab: Box::new(move |backward| {
                        if backward {
                            events.get().previous.get_untracked().focus();
                        } else {
                            events.get().next.focus();
                        }
                    }),
                })
                .flex_1(),
        )
        .child(events.get().emoji.as_ref().map_or_else(
            || {
                ui::player_icon_button(ui::Icon::Smile, move || tr("player.danmakuEmoji"))
                    .size(px(24.0))
                    .disabled(true)
                    .into_element()
            },
            |emoji| emoji(),
        ))
        .child(
            ui::player_send_action(move || tr("player.interact.send"), variant)
                .disabled(move || disabled.get() || draft.text.get().trim().is_empty())
                .on_press(move || (events.get().send)()),
        );
    match crate::player::chrome::Chrome::current() {
        Some(chrome) => chrome.bind_focus(chrome.bind_toolbar(content)),
        None => content,
    }
}

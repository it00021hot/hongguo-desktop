use nsh_core::{element::AnyElement, prelude::*};
use std::rc::Rc;

#[derive(Clone)]
struct Content(Rc<dyn Fn() -> AnyElement>);

impl PartialEq for Content {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy)]
pub(super) struct ToolbarSlot(Signal<Option<Content>>);

impl ToolbarSlot {
    pub(super) fn provide() -> Self {
        let slot = Self(signal(None));
        provide_context(slot);
        slot
    }

    pub(super) fn occupied(self) -> bool {
        self.0.with(Option::is_some)
    }
    pub(super) fn content(self) -> Option<AnyElement> {
        self.0.get().map(|content| (content.0)())
    }
}

/// Attaches the active page's controls to its shell. An inactive retained page
/// yields the slot; disposal only clears the entry that this page owns.
pub(crate) fn toolbar_slot<E: IntoElement>(
    active: impl IntoReactive<bool>,
    render: impl Fn() -> E + 'static,
) {
    let Some(slot) = use_context::<ToolbarSlot>() else {
        return;
    };
    let active = active.into_reactive();
    let content = Content(Rc::new(move || render().into_element()));
    let owned = content.clone();
    effect(move || {
        if active.get() {
            slot.0.set(Some(owned.clone()));
        } else if slot.0.get_untracked().as_ref() == Some(&owned) {
            slot.0.set(None);
        }
    });
    cleanup(move || {
        if slot.0.try_get().flatten().as_ref() == Some(&content) {
            slot.0.try_set(None);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        navigation::{Navigation, Route},
        ui,
    };
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;

    #[component]
    fn Host() -> impl IntoElement {
        let nav = Navigation::new();
        let pages = view().child(each(
            nav.routes(),
            |route: &Route| format!("{route:?}"),
            move |route, _| {
                if route.get_untracked() == Route::Rank {
                    toolbar_slot(
                        move || {
                            nav.routes()
                                .with(|routes| routes.last() == Some(&Route::Rank))
                        },
                        || text("Rank toolbar"),
                    );
                }
                view().child(
                    ui::button("Open detail", ui::ButtonVariant::Primary)
                        .on_press(move || nav.push(Route::Detail("fixture".into()))),
                )
            },
        ));
        ui::shell(nav, text("Default toolbar"), pages)
    }

    #[test]
    fn retained_pages_yield_and_restore_the_toolbar_without_leaving_disposed_controls()
    -> Result<(), String> {
        let root = Root::of::<Host>()
            .tokens(nsh_base::tokens_of(Some(include_str!("../../theme.json"))).unwrap());
        let mut config = EngineConfig::new(root);
        config.environment.os = nsh_core::platform::Os::MacOs;
        config.viewport = nsh_engine::Size::new(px(1280.0), px(840.0));
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("Default toolbar")?;
        app.press("role:button[Rankings]")?;
        app.wait_for("Rank toolbar")?;
        assert!(!app.shows("Default toolbar"));
        app.press("role:button[Settings]")?;
        app.wait_for("Default toolbar")?;
        app.press("role:button[Back]")?;
        app.wait_for("Rank toolbar")?;
        app.press("role:button[Discover]")?;
        app.wait_for("Default toolbar")?;
        assert!(!app.shows("Rank toolbar"));
        app.press("role:button[Rankings]")?;
        app.wait_for("Rank toolbar")?;
        Ok(())
    }
}

use nsh_core::{element::AnyElement, prelude::*};
use std::rc::Rc;

#[derive(Clone)]
struct Content {
    render: Rc<dyn Fn() -> AnyElement>,
}

impl PartialEq for Content {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.render, &other.render)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ToolbarSlot(Signal<Option<Content>>);

impl ToolbarSlot {
    pub(crate) fn provide() -> Self {
        let slot = Self(signal(None));
        provide_context(slot);
        slot
    }

    pub(crate) fn content(self) -> Option<AnyElement> {
        self.0.get().map(|content| (content.render)())
    }
}

/// Attaches the active page's controls to its shell. An inactive retained page
/// yields the slot; disposal only clears the entry that this page owns.
pub(crate) fn toolbar_slot<E: IntoElement>(
    active: impl IntoReactive<bool>,
    render: impl Fn() -> E + 'static,
) {
    attach(active, render);
}

fn attach<E: IntoElement>(active: impl IntoReactive<bool>, render: impl Fn() -> E + 'static) {
    let Some(slot) = use_context::<ToolbarSlot>() else {
        return;
    };
    let active = active.into_reactive();
    let content = Content {
        render: Rc::new(move || render().into_element()),
    };
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

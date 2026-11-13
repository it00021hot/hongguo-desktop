use super::{Editor, FieldOptions};
use nsh_core::prelude::*;
use std::{rc::Rc, sync::atomic::Ordering};

pub(super) fn field(editor: Editor, options: FieldOptions) -> View {
    let session = editor.session();
    session.set_node_views(crate::ui::emoji_editor_views(options.skin));
    let controls = editor.inner.with(|inner| inner.controls.clone());
    controls
        .single_line
        .store(!options.multiline, Ordering::Release);
    let binding = editor.inner.with(|inner| {
        inner.next_binding += 1;
        inner.next_binding
    });
    controls.binding.store(binding, Ordering::Release);
    let callbacks = controls.clone();
    let (view, reveal) = crate::ui::emoji_editor(
        session,
        editor.reference,
        options.placeholder,
        options.disabled,
        options.multiline,
        options.skin,
        move || {
            if callbacks.binding.load(Ordering::Acquire) == binding {
                editor.sync();
            }
        },
    );
    editor.inner.with(|inner| {
        inner.handler = Some((
            binding,
            Rc::new(super::Handlers {
                submit: Rc::from(options.submit),
                on_tab: Rc::from(options.on_tab),
                reveal,
            }),
        ));
    });
    let disabled = options.disabled;
    let current = controls.clone();
    effect(move || current.disabled.store(disabled.get(), Ordering::Release));
    // Cleanup may run after the draft's owning scope has disposed its Stored.
    // This independent handle invalidates only this mount, without reading UI state.
    cleanup(move || {
        if controls
            .binding
            .compare_exchange(binding, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            controls.disabled.store(true, Ordering::Release);
        }
    });
    view
}

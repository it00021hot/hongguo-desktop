//! A real horizontal viewport for a single-line rich document.
use nsh_base::rich_editor::view::EditorSession;
use nsh_core::prelude::*;
use std::rc::Rc;

pub(super) fn mount(
    editor: AnyElement,
    session: EditorSession,
    multiline: bool,
) -> (AnyElement, Rc<dyn Fn()>) {
    if multiline {
        return (editor, Rc::new(|| {}));
    }
    let reference = element_ref();
    let width = signal(px(0.0));
    let offset = signal(px(0.0));
    let scheduled = signal(false);
    let reveal: Rc<dyn Fn()> = Rc::new(move || {
        if scheduled.get_untracked() {
            return;
        }
        scheduled.set(true);
        let session = session.clone();
        spawn(async move {
            next_frame().await;
            scheduled.set(false);
            let width = width.get_untracked();
            if width <= px(0.0) {
                return;
            }
            let state = session.state();
            let Some(caret) = session.coords_at_pos(state.selection().head()) else {
                return;
            };
            let old = offset.get_untracked();
            let inset = session
                .coords_at_pos(1)
                .map_or(px(0.0), |first| first.origin.x.max(px(0.0)));
            let left = (caret.origin.x - inset).max(px(0.0));
            let right = caret.origin.x + caret.size.width + inset;
            let x = if left < old {
                left
            } else if right > old + width {
                right - width
            } else {
                return;
            };
            reference.scroll_to(Point::new(x.max(px(0.0)), px(0.0)), false);
        });
    });
    let layout_reveal = reveal.clone();
    let scroll = scroll()
        .element_ref(&reference)
        .horizontal()
        .scrollbar(false)
        .size_full()
        .on_layout(move |bounds: &Bounds<Points>| {
            width.set(bounds.size.width);
            layout_reveal();
        })
        .on_scroll(move |event: &ScrollEvent| offset.set(event.offset.x))
        .child(editor);
    (scroll.into_element(), reveal)
}

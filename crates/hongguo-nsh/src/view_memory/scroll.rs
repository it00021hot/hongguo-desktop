use nsh_core::prelude::*;

pub(crate) fn remember_scroll<S: Clone + PartialEq + 'static, T: Scrollable>(
    element: T,
    name: &'static str,
    scope: Memo<S>,
) -> T {
    let current = scope.get_untracked();
    let position =
        super::scope(name).signal("position", (current.clone(), Point::new(px(0.0), px(0.0))));
    let initial = position.with_untracked(|(saved, offset)| {
        if *saved == current {
            *offset
        } else {
            Point::new(px(0.0), px(0.0))
        }
    });
    let reference = element_ref();
    effect(move || {
        let current = scope.get();
        if position.with_untracked(|(saved, _)| *saved != current) {
            let zero = Point::new(px(0.0), px(0.0));
            position.set((current, zero));
            reference.scroll_to(zero, false);
        }
    });
    element
        .element_ref(&reference)
        .content_offset(initial)
        .on_scroll(move |event: &ScrollEvent| {
            position.set((scope.get_untracked(), event.offset));
        })
}

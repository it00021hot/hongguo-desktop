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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{ButtonVariant, button};
    use nsh_engine::{EngineConfig, Root, Size};
    use nsh_testing::Headless;
    use std::time::Duration;

    #[derive(Clone, Copy)]
    struct Filter(Signal<bool>);

    #[component]
    fn List() -> impl IntoElement {
        let filter = use_context::<Filter>().unwrap().0;
        remember_scroll(scroll(), "fixture-scroll", computed(move || filter.get()))
            .test_id("remembered-list")
            .flex_1()
            .child(view().children(
                (0..40).map(|index| text(format!("Row {index:02}")).h(px(40.0)).flex_shrink_0()),
            ))
    }

    #[component]
    fn Window() -> impl IntoElement {
        super::super::provide();
        let shown = signal(true);
        let filter = signal(false);
        provide_context(Filter(filter));
        view()
            .flex_1()
            .child(button("Leave list", ButtonVariant::Quiet).on_press(move || shown.set(false)))
            .child(button("Return to list", ButtonVariant::Quiet).on_press(move || shown.set(true)))
            .child(
                button("Change filter", ButtonVariant::Quiet)
                    .on_press(move || filter.update(|value| *value = !*value)),
            )
            .child(when(shown, || List::element(())))
    }

    fn start() -> Result<Headless, String> {
        let mut config = EngineConfig::new(Root::of::<Window>());
        config.viewport = Size::new(px(600.0), px(400.0));
        let mut app = Headless::start(config).map_err(|error| error.to_string())?;
        app.wait_for("text:Row 00")?;
        Ok(app)
    }

    #[test]
    fn return_restores_scroll_and_changing_filters_resets_it() -> Result<(), String> {
        let mut app = start()?;
        app.scroll("id:remembered-list", px(800.0))?;
        app.press("Leave list")?;
        app.wait_for_gone("id:remembered-list", Duration::from_secs(1))?;
        app.press("Return to list")?;
        app.wait_for("id:remembered-list")?;
        app.scroll("id:remembered-list", px(-800.0))?;
        assert!(app.scroll("id:remembered-list", px(-1.0)).is_err());
        app.scroll("id:remembered-list", px(800.0))?;
        app.press("Change filter")?;
        app.wait(Duration::from_millis(32))?;
        assert!(app.scroll("id:remembered-list", px(-1.0)).is_err());
        let mut another = start()?;
        assert!(another.scroll("id:remembered-list", px(-1.0)).is_err());
        Ok(())
    }
}

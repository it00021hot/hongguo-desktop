use nsh_core::prelude::*;
use std::future::Future;

pub(super) fn observe<F: Future<Output = Result<(), String>> + 'static>(
    pending: Signal<Option<f64>>,
    active: impl Fn() -> bool + 'static,
    wait: impl Fn(f64) -> F + 'static,
) -> Resource<f64, String> {
    resource(
        move || active().then(|| pending.get()).flatten(),
        move |target, _| {
            let waiting = wait(target);
            async move {
                waiting.await?;
                Ok(target)
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    #[component]
    fn PausedSeek() -> impl IntoElement {
        let target = signal(None::<f64>);
        let active = signal(true);
        let publication = Rc::new(RefCell::new(None::<tokio::sync::oneshot::Sender<()>>));
        let waiter = publication.clone();
        let ready = observe(
            target,
            move || active.get(),
            move |_| {
                let (publish, wait) = tokio::sync::oneshot::channel();
                *waiter.borrow_mut() = Some(publish);
                async move { wait.await.map_err(|error| error.to_string()) }
            },
        );
        let requested = signal(None::<f64>);
        let count = signal(0_u32);
        let cancelled = signal(false);
        effect(move || {
            if active.get()
                && !ready.loading()
                && ready.error().is_none()
                && let Some(at) = target.get()
                && ready.with(|ready| ready.is_some_and(|ready| *ready == at))
            {
                requested.set(Some(at));
                count.update(|count| *count += 1);
            }
        });
        view()
            .child(text(move || {
                format!("paused:{:?}:{}", requested.get(), count.get())
            }))
            .child(text(move || format!("cancelled:{}", cancelled.get())))
            .child(text(move || format!("waiting:{}", ready.loading())))
            .child(
                crate::ui::button("Seek42", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || target.set(Some(42.0))),
            )
            .child(
                crate::ui::button("Seek84", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || target.set(Some(84.0))),
            )
            .child(
                crate::ui::button("Leave", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || active.set(false)),
            )
            .child({
                let publication = publication.clone();
                crate::ui::button("Inspect", crate::ui::ButtonVariant::Quiet).on_press(move || {
                    cancelled.set(
                        publication
                            .borrow()
                            .as_ref()
                            .is_some_and(tokio::sync::oneshot::Sender::is_closed),
                    );
                })
            })
            .child(
                crate::ui::button("Publish", crate::ui::ButtonVariant::Quiet).on_press(move || {
                    if let Some(publish) = publication.borrow_mut().take() {
                        let _ = publish.send(());
                    }
                }),
            )
    }

    #[test]
    fn a_paused_seek_wakes_when_the_producer_publishes_without_native_ticks() -> Result<(), String>
    {
        let mut app =
            nsh_testing::Headless::start(nsh_engine::EngineConfig::new(nsh_engine::Root::of::<
                PausedSeek,
            >()))
            .map_err(|error| error.to_string())?;
        app.wait_for("text:paused:None:0")?;
        app.press("Seek42")?;
        app.press("Publish")?;
        app.wait_for("text:paused:Some(42.0):1")?;
        Ok(())
    }

    #[test]
    fn a_retained_ready_value_cannot_dispatch_a_replacement_that_is_still_waiting()
    -> Result<(), String> {
        let mut app =
            nsh_testing::Headless::start(nsh_engine::EngineConfig::new(nsh_engine::Root::of::<
                PausedSeek,
            >()))
            .map_err(|error| error.to_string())?;
        app.wait_for("text:paused:None:0")?;
        app.press("Seek42")?;
        app.press("Publish")?;
        app.wait_for("text:paused:Some(42.0):1")?;
        app.press("Seek84")?;
        app.wait_for("text:waiting:true")?;
        app.press("Seek42")?;
        assert!(app.screen().shows("paused:Some(42.0):1"));
        app.press("Publish")?;
        app.wait_for("text:paused:Some(42.0):2")?;
        Ok(())
    }

    #[test]
    fn leaving_the_source_cancels_its_wait_even_without_a_native_tick() -> Result<(), String> {
        let mut app =
            nsh_testing::Headless::start(nsh_engine::EngineConfig::new(nsh_engine::Root::of::<
                PausedSeek,
            >()))
            .map_err(|error| error.to_string())?;
        app.wait_for("text:paused:None:0")?;
        app.press("Seek42")?;
        app.wait_for("text:waiting:true")?;
        app.press("Leave")?;
        app.press("Inspect")?;
        app.wait_for("text:cancelled:true")?;
        assert!(app.screen().shows("paused:None:0"));
        Ok(())
    }
}

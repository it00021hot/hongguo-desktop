use crate::{bridge::Failure, i18n::tr, ui};
use nsh_core::prelude::*;

fn may_retry(error: Option<&Failure>) -> bool {
    error.is_none_or(Failure::can_retry)
}

pub(super) fn retry(error: Option<&Failure>, action: impl FnOnce()) {
    if may_retry(error) {
        action();
    }
}

pub(super) fn panel(
    error: Memo<Option<Failure>>,
    fallback: Signal<SharedString>,
    on_retry: impl Fn() + Copy + 'static,
) -> View {
    ui::status(move || {
        error
            .get()
            .map_or_else(|| fallback.get(), |error| error.text())
    })
    .child(when(
        move || error.with(|error| may_retry(error.as_ref())),
        move || {
            ui::button(move || tr("common.retry"), ui::ButtonVariant::Secondary).on_press(on_retry)
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hongguo_common::AppError;
    use nsh_engine::{EngineConfig, Root};
    use nsh_testing::Headless;

    #[component]
    fn PlaybackFailure() -> impl IntoElement {
        let error = signal(Some(Failure::from(AppError::CodecUnsupported(
            "h264".into(),
        ))));
        let attempts = signal(0_u32);
        let attempt = move || {
            retry(error.get_untracked().as_ref(), || {
                attempts.update(|count| *count += 1)
            })
        };
        view()
            .child(panel(
                computed(move || error.get()),
                signal(SharedString::default()),
                attempt,
            ))
            .child(text(move || format!("attempts:{}", attempts.get())))
            .child(
                ui::button("Invoke callback directly", ui::ButtonVariant::Quiet).on_press(attempt),
            )
            .child(
                ui::button("Use operational error", ui::ButtonVariant::Quiet).on_press(move || {
                    error.set(Some(Failure::from(AppError::Network("offline".into()))))
                }),
            )
    }

    #[test]
    fn terminal_codec_errors_hide_retry_and_reject_direct_callback_invocation() -> Result<(), String>
    {
        let mut app = Headless::start(EngineConfig::new(Root::of::<PlaybackFailure>()))
            .map_err(|error| error.to_string())?;
        app.wait_for("text:attempts:0")?;
        assert!(!app.screen().matches("role:button[Retry]")?);
        app.press("Invoke callback directly")?;
        app.wait_for("text:attempts:0")?;
        app.press("Use operational error")?;
        app.wait_for("role:button[Retry]")?;
        app.press("role:button[Retry]")?;
        app.wait_for("text:attempts:1")?;
        Ok(())
    }
}

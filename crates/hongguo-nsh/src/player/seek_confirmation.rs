use nsh_core::prelude::*;
use std::time::Duration;

pub(super) const RATE: f32 = 0.25;
pub(super) const PROGRESS_INTERVAL: f32 = 50.0;
pub(super) const NORMAL_PROGRESS_INTERVAL: f32 = 250.0;
const DEADLINE: Duration = Duration::from_secs(3);

pub(super) fn observe(
    issued: Signal<Option<f64>>,
    pending: Signal<Option<f64>>,
    paused: Signal<bool>,
    allowed: impl Fn() -> bool + 'static,
    expired: impl Fn() + Copy + 'static,
) -> Memo<bool> {
    let active = computed(move || {
        issued.get().is_some() && issued.get() == pending.get() && paused.get() && allowed()
    });
    effect(move || {
        if !active.get() {
            return;
        }
        let target = issued.get();
        let deadline = set_timeout(DEADLINE, move || {
            if active.get_untracked() && issued.get_untracked() == target {
                expired();
            }
        });
        cleanup(move || deadline.cancel());
    });
    active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[component]
    fn Confirmation() -> impl IntoElement {
        let issued = signal(None::<f64>);
        let pending = signal(None::<f64>);
        let paused = signal(true);
        let rate = signal(1.5_f32);
        let allowed = signal(true);
        let failures = signal(0_u32);
        let driving = observe(
            issued,
            pending,
            paused,
            move || allowed.get(),
            move || {
                failures.update(|failures| *failures += 1);
                pending.set(None);
            },
        );
        let button = |label, action: Box<dyn Fn()>| {
            crate::ui::button(label, crate::ui::ButtonVariant::Quiet).on_press(action)
        };
        view()
            .child(text(move || {
                format!(
                    "drive:{} user:{} rate:{} failures:{}",
                    driving.get(),
                    paused.get(),
                    rate.get(),
                    failures.get()
                )
            }))
            .child(button("Request", Box::new(move || pending.set(Some(42.0)))))
            .child(button("Dispatch", Box::new(move || issued.set(Some(42.0)))))
            .child(button(
                "Retarget",
                Box::new(move || pending.set(Some(84.0))),
            ))
            .child(button("Confirm", Box::new(move || pending.set(None))))
            .child(button("Leave", Box::new(move || allowed.set(false))))
            .child(button("Play", Box::new(move || paused.set(false))))
    }

    fn app() -> Result<nsh_testing::Headless, String> {
        let mut config = nsh_engine::EngineConfig::new(nsh_engine::Root::of::<Confirmation>());
        config.clock = nsh_core::time::Clock::manual(std::time::SystemTime::UNIX_EPOCH);
        nsh_testing::Headless::start(config).map_err(|error| error.to_string())
    }

    #[test]
    fn confirmation_starts_only_after_dispatch_and_preserves_the_users_pause_and_rate()
    -> Result<(), String> {
        let mut app = app()?;
        app.wait_for("text:drive:false user:true rate:1.5 failures:0")?;
        app.press("Request")?;
        assert!(
            app.screen()
                .matches("text:drive:false user:true rate:1.5 failures:0")?
        );
        app.press("Dispatch")?;
        app.wait_for("text:drive:true user:true rate:1.5 failures:0")?;
        app.press("Confirm")?;
        app.wait_for("text:drive:false user:true rate:1.5 failures:0")?;
        app.run_for(DEADLINE)?;
        assert!(
            app.screen()
                .matches("text:drive:false user:true rate:1.5 failures:0")?
        );
        Ok(())
    }

    #[test]
    fn an_unconfirmed_seek_expires_once_without_resending_or_changing_preferences()
    -> Result<(), String> {
        let mut app = app()?;
        app.wait_for("text:drive:false user:true rate:1.5 failures:0")?;
        app.press("Request")?;
        app.press("Dispatch")?;
        app.run_for(DEADLINE + Duration::from_millis(1))?;
        app.wait_for("text:drive:false user:true rate:1.5 failures:1")?;
        app.run_for(DEADLINE)?;
        assert!(
            app.screen()
                .matches("text:drive:false user:true rate:1.5 failures:1")?
        );
        Ok(())
    }

    #[test]
    fn retargeting_leaving_or_manual_play_closes_the_old_confirmation_window() -> Result<(), String>
    {
        for action in ["Retarget", "Leave", "Play"] {
            let mut app = app()?;
            app.wait_for("text:drive:false user:true rate:1.5 failures:0")?;
            app.press("Request")?;
            app.press("Dispatch")?;
            app.wait_for("text:drive:true user:true rate:1.5 failures:0")?;
            app.press(action)?;
            app.run_for(DEADLINE)?;
            assert!(app.screen().matches(if action == "Play" {
                "text:drive:false user:false rate:1.5 failures:0"
            } else {
                "text:drive:false user:true rate:1.5 failures:0"
            })?);
        }
        Ok(())
    }
}

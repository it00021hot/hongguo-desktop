#[derive(Debug, PartialEq)]
pub(super) struct DefinitionChange {
    pub definition: Option<u32>,
    pub resume_at: f64,
    pub reset_definition: bool,
}

pub(super) fn change_definition(
    current: Option<u32>,
    requested: Option<u32>,
    at: f64,
) -> Option<DefinitionChange> {
    (current != requested).then_some(DefinitionChange {
        definition: requested,
        reset_definition: requested.is_none(),
        resume_at: if at.is_finite() && at >= 0.0 { at } else { 0.0 },
    })
}

pub(super) fn change_episode(current: u32, requested: u32) -> bool {
    current != requested
}

pub(super) fn initial_seek(resume_at: f64, growing: bool) -> Option<f64> {
    (resume_at.is_finite() && resume_at >= 0.0 && (growing || resume_at > 0.0)).then_some(resume_at)
}

pub(super) fn source_current(loading: bool, failed: bool, same_lease: bool) -> bool {
    !loading && !failed && same_lease
}

pub(super) fn seek_available(
    target: f64,
    native_duration: f64,
    published: Option<f64>,
    growing: bool,
) -> bool {
    target.is_finite()
        && target >= 0.0
        && (target == 0.0
            || !growing
            || available_duration(native_duration, published)
                .is_some_and(|duration| target < duration))
}

pub(super) fn seek_exceeds_complete(target: f64, native: f64, complete: bool) -> bool {
    complete && native.is_finite() && native > 0.0 && target >= native
}

fn available_duration(native: f64, published: Option<f64>) -> Option<f64> {
    std::iter::once(native)
        .chain(published)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .reduce(f64::max)
}

#[derive(Default)]
pub(super) struct SeekLatch {
    issued: Option<f64>,
}

impl SeekLatch {
    pub fn dispatch(&mut self, target: f64) -> bool {
        if self.issued == Some(target) {
            return false;
        }
        self.issued = Some(target);
        true
    }

    pub fn confirmed(&self, target: f64, actual: f64) -> bool {
        const NATIVE_CONFIRMATION_TOLERANCE: f64 = 1.0;
        self.issued == Some(target)
            && actual.is_finite()
            && actual >= 0.0
            && (actual - target).abs() <= NATIVE_CONFIRMATION_TOLERANCE
    }

    pub fn clear(&mut self) {
        self.issued = None;
    }
}

#[derive(Default)]
pub(super) struct LoadLatch {
    loaded: bool,
}

impl LoadLatch {
    pub fn seek(&mut self, pending: Option<f64>, saved: f64, duration: f64) -> Option<f64> {
        if self.loaded {
            return None;
        }
        self.loaded = true;
        let at = pending.unwrap_or(saved);
        Some(
            if at.is_finite()
                && at >= 0.0
                && (!duration.is_finite() || duration <= 0.0 || at < duration)
            {
                at
            } else {
                0.0
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_core::prelude::*;

    #[test]
    fn growing_zero_start_requires_a_real_seek_instead_of_the_native_live_edge() {
        assert_eq!(initial_seek(0.0, true), Some(0.0));
        assert_eq!(initial_seek(0.0, false), None);
        for growing in [false, true] {
            assert_eq!(initial_seek(37.0, growing), Some(37.0));
            for invalid in [-1.0, f64::NAN, f64::INFINITY] {
                assert_eq!(initial_seek(invalid, growing), None);
            }
        }
    }

    #[component]
    fn RejectedReplacement() -> impl IntoElement {
        let reject = signal(false);
        let source = resource_with_value(
            "old".to_string(),
            move || reject.get().then_some(()),
            |_, _| async { Err::<String, String>("rejected".into()) },
        );
        view()
            .child(
                crate::ui::button("Reject replacement", crate::ui::ButtonVariant::Quiet)
                    .on_press(move || reject.set(true)),
            )
            .child(text(move || {
                let value = source.with(|value| value.cloned().unwrap_or_default());
                let failed = source.error().is_some();
                format!(
                    "{}:{value}:{}",
                    if failed { "rejected" } else { "ready" },
                    source_current(source.loading(), failed, value == "old")
                )
            }))
    }

    #[test]
    fn a_rejected_resource_retains_its_value_but_cannot_resume_the_old_video() -> Result<(), String>
    {
        let mut app =
            nsh_testing::Headless::start(nsh_engine::EngineConfig::new(nsh_engine::Root::of::<
                RejectedReplacement,
            >()))
            .map_err(|error| error.to_string())?;
        app.wait_for("text:ready:old:true")?;
        app.press("Reject replacement")?;
        app.wait_for("text:rejected:old:false")?;
        Ok(())
    }

    #[test]
    fn selecting_the_current_definition_does_not_leave_a_stale_seek() {
        assert_eq!(change_definition(Some(720), Some(720), 42.0), None);
        assert_eq!(change_definition(None, None, 42.0), None);
    }

    #[test]
    fn a_definition_switch_including_auto_keeps_this_sessions_position() {
        assert_eq!(
            change_definition(Some(720), None, 42.0),
            Some(DefinitionChange {
                definition: None,
                resume_at: 42.0,
                reset_definition: true,
            })
        );
        assert_eq!(
            change_definition(None, Some(1080), 55.0),
            Some(DefinitionChange {
                definition: Some(1080),
                resume_at: 55.0,
                reset_definition: false,
            })
        );
    }

    #[test]
    fn selecting_the_current_episode_does_not_reset_the_visible_playhead() {
        assert!(!change_episode(3, 3));
        assert!(change_episode(3, 7));
    }

    #[test]
    fn metadata_updates_do_not_seek_a_running_source_back_to_its_saved_resume() {
        let mut load = LoadLatch::default();
        assert_eq!(load.seek(Some(42.0), 10.0, 168.0), Some(42.0));
        assert_eq!(load.seek(None, 10.0, 168.533), None);
    }

    #[test]
    fn completed_or_invalid_resume_points_start_at_the_beginning() {
        for saved in [168.0, 336.0, f64::NAN, -10.0] {
            assert_eq!(LoadLatch::default().seek(None, saved, 168.0), Some(0.0));
        }
    }

    #[test]
    fn a_prefix_does_not_accept_a_seek_beyond_its_native_available_length() {
        assert!(!seek_available(42.0, 0.2667, None, true));
        assert!(!seek_available(42.0, -1.0, None, true));
        assert!(seek_available(42.0, 45.0, None, true));
        assert!(seek_available(42.0, 168.0, None, false));
    }

    #[test]
    fn published_hls_time_allows_resume_while_native_duration_is_indefinite() {
        assert!(seek_available(42.0, -1.0, Some(45.0), true));
        assert!(!seek_available(42.0, -1.0, Some(10.0), true));
        assert!(seek_available(42.0, 45.0, Some(10.0), true));
        assert!(!seek_available(42.0, -1.0, Some(f64::INFINITY), true));
    }

    #[test]
    fn pending_seek_dispatches_once_and_waits_for_real_native_confirmation() {
        let mut seek = SeekLatch::default();
        assert!(!seek.confirmed(42.0, 42.0));
        assert!(seek.dispatch(42.0));
        assert!(!seek.dispatch(42.0));
        assert!(!seek.confirmed(42.0, 0.5));
        assert!(!seek.confirmed(42.0, f64::NAN));
        assert!(seek.confirmed(42.0, 41.5));
        assert!(seek.confirmed(42.0, 42.8));
        assert!(seek.dispatch(70.0));
        assert!(!seek.confirmed(70.0, 42.8));
        seek.clear();
        assert!(seek.dispatch(70.0));
    }

    #[test]
    fn a_finished_source_rejects_unreachable_resume_instead_of_waiting_forever() {
        assert!(!seek_exceeds_complete(42.0, -1.0, false));
        assert!(!seek_exceeds_complete(42.0, -1.0, true));
        assert!(!seek_exceeds_complete(42.0, 45.0, true));
        assert!(seek_exceeds_complete(45.0, 45.0, true));
    }
}

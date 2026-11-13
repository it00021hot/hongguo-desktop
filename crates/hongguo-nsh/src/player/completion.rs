#[derive(Clone, Copy)]
pub(super) struct Evidence {
    pub loaded: bool,
    pub growing: bool,
    pub producer_complete: bool,
    pub position: f64,
    pub native_duration: f64,
    pub expected_duration: Option<f64>,
    pub published_duration: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum End {
    Ignore,
    AwaitMore,
    Complete,
    Repeat,
}

#[derive(Default)]
pub(super) struct Gate {
    completed: bool,
}

impl Gate {
    #[cfg(test)]
    pub fn on_end(&mut self, evidence: Evidence) -> End {
        self.on_end_for(evidence, false)
    }

    pub fn on_end_for(&mut self, evidence: Evidence, repeat: bool) -> End {
        if self.completed || !evidence.loaded {
            return End::Ignore;
        }
        if evidence.growing {
            let expected = evidence
                .expected_duration
                .filter(|value| value.is_finite() && *value > 0.0)
                .or_else(|| {
                    evidence
                        .producer_complete
                        .then_some(evidence.published_duration)
                        .flatten()
                        .filter(|value| value.is_finite() && *value > 0.0)
                });
            let native = (evidence.native_duration.is_finite() && evidence.native_duration > 0.0)
                .then_some(evidence.native_duration);
            let duration = match (expected, native) {
                (Some(expected), Some(native)) => Some(expected.max(native)),
                (expected, native) => expected.or(native),
            };
            // Episode metadata is rounded to whole seconds; native progress is sampled.
            // A one-second end margin covers rounding without accepting a short prefix.
            if !evidence.producer_complete
                || !evidence.position.is_finite()
                || evidence.position < 0.0
                || duration.is_none_or(|duration| evidence.position + 1.0 < duration)
            {
                return End::AwaitMore;
            }
        }
        if repeat {
            return End::Repeat;
        }
        self.completed = true;
        End::Complete
    }
}

pub(super) fn end_is_blocked(user_paused: bool, pending_seek: bool, confirming: bool) -> bool {
    user_paused || pending_seek || confirming
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_publication_can_complete_unknown_native_and_api_duration_without_accepting_a_prefix()
    {
        let final_frame = Evidence {
            expected_duration: None,
            native_duration: 0.0,
            published_duration: Some(168.533),
            position: 168.5,
            ..prefix(0.0, true)
        };
        assert_eq!(Gate::default().on_end(final_frame), End::Complete);
        assert_eq!(
            Gate::default().on_end(Evidence {
                producer_complete: false,
                ..final_frame
            }),
            End::AwaitMore
        );
        assert_eq!(
            Gate::default().on_end(Evidence {
                position: 0.2667,
                ..final_frame
            }),
            End::AwaitMore
        );
        assert_eq!(
            Gate::default().on_end(Evidence {
                published_duration: Some(f64::NAN),
                ..final_frame
            }),
            End::AwaitMore
        );
        assert_eq!(
            Gate::default().on_end(Evidence {
                published_duration: Some(0.2667),
                ..prefix(0.2667, true)
            }),
            End::AwaitMore
        );
    }

    #[test]
    fn reading_comments_repeats_without_consuming_completion_or_accepting_prefix() {
        let mut gate = Gate::default();
        assert_eq!(gate.on_end_for(prefix(0.2667, false), true), End::AwaitMore);
        let end = Evidence {
            position: 168.5,
            native_duration: 168.533,
            ..prefix(0.0, true)
        };
        assert_eq!(gate.on_end_for(end, true), End::Repeat);
        assert_eq!(gate.on_end_for(end, true), End::Repeat);
        assert_eq!(gate.on_end_for(end, false), End::Complete);
        assert_eq!(gate.on_end_for(end, true), End::Ignore);
    }

    fn prefix(position: f64, complete: bool) -> Evidence {
        Evidence {
            loaded: true,
            growing: true,
            producer_complete: complete,
            position,
            native_duration: 0.2667,
            expected_duration: Some(168.0),
            published_duration: None,
        }
    }

    #[test]
    fn an_open_prefix_is_not_the_end_of_the_episode() {
        assert_eq!(
            Gate::default().on_end(prefix(0.2667, false)),
            End::AwaitMore
        );
    }

    #[test]
    fn producer_completion_does_not_validate_an_early_native_end() {
        assert_eq!(Gate::default().on_end(prefix(0.2667, true)), End::AwaitMore);
    }

    #[test]
    fn a_real_end_can_complete_only_once() {
        let mut gate = Gate::default();
        let evidence = Evidence {
            position: 168.5,
            native_duration: 168.533,
            ..prefix(0.0, true)
        };
        assert_eq!(gate.on_end(evidence), End::Complete);
        assert_eq!(gate.on_end(evidence), End::Ignore);
    }

    #[test]
    fn an_unloaded_or_invalid_native_end_cannot_advance() {
        assert_eq!(
            Gate::default().on_end(Evidence {
                loaded: false,
                ..prefix(0.0, true)
            }),
            End::Ignore
        );
        assert_eq!(
            Gate::default().on_end(Evidence {
                position: f64::NAN,
                ..prefix(0.0, true)
            }),
            End::AwaitMore
        );
    }

    #[test]
    fn unknown_metadata_still_waits_for_the_producer() {
        assert_eq!(
            Gate::default().on_end(Evidence {
                expected_duration: None,
                ..prefix(0.2667, false)
            }),
            End::AwaitMore
        );
    }

    #[test]
    fn a_complete_ordinary_source_uses_its_native_end() {
        assert_eq!(
            Gate::default().on_end(Evidence {
                growing: false,
                producer_complete: true,
                loaded: true,
                position: 167.9,
                native_duration: 168.533,
                expected_duration: Some(168.0),
                published_duration: None,
            }),
            End::Complete
        );
    }

    #[test]
    fn a_paused_confirmation_then_queued_end_cannot_consume_the_completion_gate() {
        let mut seek = crate::player::transport::SeekLatch::default();
        seek.dispatch(168.0);
        assert!(seek.confirmed(168.0, 168.5));
        let mut gate = Gate::default();
        let ended = prefix(168.5, true);
        let outcome = if end_is_blocked(true, false, false) {
            End::Ignore
        } else {
            gate.on_end(ended)
        };
        assert_eq!(outcome, End::Ignore);
        assert!(!end_is_blocked(false, false, false));
        assert_eq!(gate.on_end(ended), End::Complete);
    }
}

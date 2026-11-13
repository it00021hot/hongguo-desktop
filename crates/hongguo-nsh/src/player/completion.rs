#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum End {
    Ignore,
    Complete,
    Repeat,
}

#[derive(Default)]
pub(super) struct Gate {
    completed: bool,
}

impl Gate {
    /// Only the audio player's actual end callback enters this gate. A decoder
    /// queue reaching EOF is not playback completion.
    pub fn on_end_for(&mut self, loaded: bool, repeat: bool) -> End {
        if self.completed || !loaded {
            return End::Ignore;
        }
        if repeat {
            return End::Repeat;
        }
        self.completed = true;
        End::Complete
    }
}

pub(super) fn end_is_blocked(user_paused: bool, pending_seek: bool) -> bool {
    user_paused || pending_seek
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unloaded_end_does_not_consume_the_completion_gate() {
        let mut gate = Gate::default();
        assert_eq!(gate.on_end_for(false, false), End::Ignore);
        assert_eq!(gate.on_end_for(true, false), End::Complete);
        assert_eq!(gate.on_end_for(true, false), End::Ignore);
    }

    #[test]
    fn reading_comments_repeats_without_consuming_completion() {
        let mut gate = Gate::default();
        assert_eq!(gate.on_end_for(true, true), End::Repeat);
        assert_eq!(gate.on_end_for(true, true), End::Repeat);
        assert_eq!(gate.on_end_for(true, false), End::Complete);
        assert_eq!(gate.on_end_for(true, true), End::Ignore);
    }

    #[test]
    fn a_paused_or_seeking_source_cannot_advance_from_a_queued_end() {
        assert!(end_is_blocked(true, false));
        assert!(end_is_blocked(false, true));
        assert!(!end_is_blocked(false, false));
    }
}

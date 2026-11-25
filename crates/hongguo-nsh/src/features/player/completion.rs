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

pub(super) fn end_is_blocked(user_paused: bool, pending_seek: bool, handoff: bool) -> bool {
    user_paused || pending_seek || handoff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_is_blocked_during_handoff_so_native_end_cannot_black_the_cut() {
        assert!(end_is_blocked(false, false, true));
        assert!(end_is_blocked(true, false, false));
        assert!(end_is_blocked(false, true, false));
        assert!(!end_is_blocked(false, false, false));
    }
}

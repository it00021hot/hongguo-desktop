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

pub(super) fn end_is_blocked(
    user_paused: bool,
    pending_seek: Option<f64>,
    duration: f64,
    handoff: bool,
) -> bool {
    user_paused
        || handoff
        || pending_seek.is_some_and(|target| !super::transport::tail_seek(target, duration))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_is_blocked_during_handoff_so_native_end_cannot_black_the_cut() {
        assert!(end_is_blocked(false, None, 64.21, true));
        assert!(end_is_blocked(true, None, 64.21, false));
        assert!(end_is_blocked(false, Some(6.0), 64.21, false));
        assert!(!end_is_blocked(false, None, 64.21, false));
        assert!(!end_is_blocked(false, Some(63.96), 64.21, false));
    }
}

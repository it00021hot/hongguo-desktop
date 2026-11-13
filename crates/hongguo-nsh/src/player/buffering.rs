#[derive(Default)]
pub(super) struct Buffering {
    native: bool,
}

impl Buffering {
    pub fn native_event(&mut self, native: bool, seeking: bool) -> bool {
        self.native = native;
        self.is_buffering(seeking)
    }

    pub fn progress(&mut self, confirmed: bool, advanced: bool, seeking: bool) -> Option<bool> {
        if (!confirmed && !advanced) || seeking {
            return None;
        }
        Some(self.is_buffering(seeking))
    }

    pub fn is_buffering(&self, seeking: bool) -> bool {
        self.native || seeking
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paused_backwards_confirmation_clears_waiting_even_if_buffer_false_arrived_first() {
        let mut buffering = Buffering::default();
        assert!(buffering.native_event(false, true));
        assert_eq!(buffering.progress(true, false, false), Some(false));
        assert!(!buffering.is_buffering(false));
    }

    #[test]
    fn confirmation_preserves_a_real_native_buffer_until_its_own_false_event() {
        let mut buffering = Buffering::default();
        assert!(buffering.native_event(true, true));
        assert_eq!(buffering.progress(true, false, false), Some(true));
        assert!(!buffering.native_event(false, false));
    }
}

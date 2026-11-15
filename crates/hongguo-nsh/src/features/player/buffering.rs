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

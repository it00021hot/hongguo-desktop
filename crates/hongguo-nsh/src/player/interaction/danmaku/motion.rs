use nsh_core::prelude::*;

/// One binding from real playback state to the caption renderer.
#[derive(Clone, Copy)]
pub(in crate::player) struct Motion {
    pub(super) frozen: Memo<bool>,
    pub(super) seek_pending: Signal<Option<f64>>,
}

impl Motion {
    pub(in crate::player) fn new(
        allowed: Memo<bool>,
        paused: Signal<bool>,
        loading: Memo<bool>,
        buffering: Signal<bool>,
        seek_pending: Signal<Option<f64>>,
    ) -> Self {
        Self {
            frozen: computed(move || {
                !allowed.get()
                    || paused.get()
                    || loading.get()
                    || buffering.get()
                    || seek_pending.get().is_some()
            }),
            seek_pending,
        }
    }
}

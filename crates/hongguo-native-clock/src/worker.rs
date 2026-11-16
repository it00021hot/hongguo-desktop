use crate::{NativePlayer, Shared};
use std::sync::{atomic::Ordering, mpsc::Receiver};

pub(super) fn run<P: NativePlayer>(
    shared: &Shared,
    receiver: Receiver<()>,
    factory: impl FnOnce() -> Result<P, String>,
) -> Result<(), String> {
    let mut player = factory()?;
    let mut revision = None;
    loop {
        if shared.closed.load(Ordering::Acquire) {
            return Ok(());
        }
        let (control, next, seek) = {
            let mut mailbox = shared
                .mailbox
                .lock()
                .map_err(|_| "native clock mailbox poisoned")?;
            (mailbox.control, mailbox.revision, mailbox.seek.take())
        };
        if revision != Some(next) {
            player.control(control)?;
            revision = Some(next);
        }
        if let Some(target) = seek {
            player.seek(target)?;
        }
        let mut position = player.position()?;
        if !position.seconds.is_finite() || position.seconds < 0.0 {
            return Err("native audio returned an invalid media time".into());
        }
        // A newer seek must not be overwritten by a query of the previous epoch.
        let mailbox = shared
            .mailbox
            .lock()
            .map_err(|_| "native clock mailbox poisoned")?;
        if mailbox.seek.is_some() {
            position.seeking = true;
            position.ready = false;
        }
        *shared
            .position
            .lock()
            .map_err(|_| "native clock state lock poisoned")? = Ok(position);
        drop(mailbox);
        if receiver.recv().is_err() {
            return Ok(());
        }
    }
}

pub(super) fn bind(active: impl Fn() -> bool + 'static) {
    #[cfg(target_os = "macos")]
    {
        use nsh_core::prelude::*;
        use std::cell::RefCell;
        use std::rc::Rc;

        // Rc 而不是 stored：owner dispose 时 cleanup 再碰 Stored 会触发
        // “signal read or written after its owner was disposed”。释放靠 Guard::Drop。
        let guard = Rc::new(RefCell::new(macos::Guard::default()));
        effect(move || {
            let keep_awake = active();
            guard.borrow_mut().set_active(keep_awake);
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = active;
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use nsh_platform_macos::{
        objc2::{rc::Retained, runtime::ProtocolObject},
        objc2_foundation::{NSActivityOptions, NSObjectProtocol, NSProcessInfo, NSString},
    };

    #[derive(Default)]
    pub(super) struct Guard {
        activity: Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
    }

    impl Guard {
        pub(super) fn set_active(&mut self, active: bool) {
            if active == self.activity.is_some() {
                return;
            }
            if !active {
                self.release();
                return;
            }
            let reason = NSString::from_str("Playing fullscreen video");
            let options = NSActivityOptions::IdleDisplaySleepDisabled
                | NSActivityOptions::IdleSystemSleepDisabled;
            self.activity = Some(
                NSProcessInfo::processInfo().beginActivityWithOptions_reason(options, &reason),
            );
        }

        fn release(&mut self) {
            if let Some(activity) = self.activity.take() {
                // SAFETY: token from beginActivityWithOptions_reason on the same process info.
                unsafe {
                    NSProcessInfo::processInfo().endActivity(&activity);
                }
            }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            self.release();
        }
    }
}

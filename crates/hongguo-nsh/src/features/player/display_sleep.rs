pub(super) fn bind(active: impl Fn() -> bool + 'static) {
    #[cfg(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "ios",
        target_os = "android"
    ))]
    {
        use nsh_core::prelude::*;
        use std::cell::RefCell;
        use std::rc::Rc;

        // Rc 而不是 stored：owner dispose 时 cleanup 再碰 Stored 会触发
        // “signal read or written after its owner was disposed”。释放靠 Guard::Drop。
        let guard = Rc::new(RefCell::new(Guard::default()));
        effect(move || {
            let keep_awake = active();
            guard.borrow_mut().set_active(keep_awake);
        });
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "ios",
        target_os = "android"
    )))]
    {
        let _ = active;
    }
}

#[cfg(target_os = "android")]
use android::Guard;
#[cfg(target_os = "ios")]
use ios::Guard;
#[cfg(target_os = "macos")]
use macos::Guard;
#[cfg(target_os = "windows")]
use windows::Guard;

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

#[cfg(target_os = "windows")]
mod windows {
    use nsh_platform_windows::windows::Win32::System::Power::{
        ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, EXECUTION_STATE,
        SetThreadExecutionState,
    };

    #[derive(Default)]
    pub(super) struct Guard {
        active: bool,
    }

    impl Guard {
        pub(super) fn set_active(&mut self, active: bool) {
            if active == self.active {
                return;
            }
            let flags = if active {
                ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED
            } else {
                ES_CONTINUOUS
            };
            // SAFETY: SetThreadExecutionState only takes execution flags; no pointers.
            let previous = unsafe { SetThreadExecutionState(flags) };
            if previous == EXECUTION_STATE(0) {
                log::warn!("[NSH player] SetThreadExecutionState failed");
                return;
            }
            self.active = active;
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if self.active {
                self.set_active(false);
            }
        }
    }
}

#[cfg(target_os = "ios")]
mod ios {
    use objc2::MainThreadMarker;
    use objc2_ui_kit::UIApplication;

    #[derive(Default)]
    pub(super) struct Guard {
        active: bool,
    }

    impl Guard {
        pub(super) fn set_active(&mut self, active: bool) {
            if active == self.active {
                return;
            }
            let Some(marker) = MainThreadMarker::new() else {
                log::warn!("[NSH player] idle timer must be changed on the main thread");
                return;
            };
            UIApplication::sharedApplication(marker).setIdleTimerDisabled(active);
            self.active = active;
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if self.active {
                self.set_active(false);
            }
        }
    }
}

#[cfg(target_os = "android")]
mod android {
    use jni::{JavaVM, objects::JValue};

    const ACTIVITY: &str = "com/hongguo/nsh/MainActivity";

    #[derive(Default)]
    pub(super) struct Guard {
        active: bool,
    }

    impl Guard {
        pub(super) fn set_active(&mut self, active: bool) {
            if active == self.active {
                return;
            }
            if let Err(error) = apply(active) {
                log::warn!("[NSH player] cannot keep the screen on: {error}");
                return;
            }
            self.active = active;
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if self.active {
                self.set_active(false);
            }
        }
    }

    fn apply(active: bool) -> Result<(), String> {
        // SAFETY: NSH sets the process JavaVM in ndk-context before app code runs.
        let vm = unsafe { JavaVM::from_raw(ndk_context::android_context().vm().cast()) }
            .map_err(|error| error.to_string())?;
        let mut env = vm
            .attach_current_thread()
            .map_err(|error| error.to_string())?;
        env.call_static_method(
            ACTIVITY,
            "setKeepScreenOn",
            "(Z)V",
            &[JValue::Bool(u8::from(active))],
        )
        .map(|_| ())
        .map_err(|error| {
            let _ = env.exception_clear();
            error.to_string()
        })
    }
}

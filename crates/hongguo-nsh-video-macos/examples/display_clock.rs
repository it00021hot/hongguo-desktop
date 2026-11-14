#[cfg(target_os = "macos")]
mod probe {
    use hongguo_nsh_video_apple::{DisplayClock, PlayerController};
    use nsh_platform_macos::{
        objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSScreen},
        objc2_foundation::{NSDate, NSObject, NSRunLoop},
    };
    use objc2::{
        DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    };
    use objc2_quartz_core::CAMetalLayer;
    use std::{
        cell::{Cell, RefCell},
        rc::{Rc, Weak},
        sync::Arc,
        time::{Duration, Instant},
    };

    struct State {
        controller: Arc<PlayerController>,
        clock: RefCell<Weak<DisplayClock>>,
        ticks: Cell<u32>,
    }
    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = State]
        struct ClockTarget;
        impl ClockTarget {
            #[unsafe(method(hongguoFrameTick:))]
            fn tick(&self, _: &objc2::runtime::AnyObject) {
                self.ivars().ticks.set(self.ivars().ticks.get() + 1);
                if let Some(clock) = self.ivars().clock.borrow().upgrade() { clock.tick(&self.ivars().controller); }
            }
        }
    );

    pub fn run() {
        let mtm = MainThreadMarker::new().expect("probe main thread");
        let application = NSApplication::sharedApplication(mtm);
        application.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
        let controller = Arc::new(PlayerController::spawn(CAMetalLayer::new(), |_| {}).unwrap());
        let allocated = ClockTarget::alloc(mtm).set_ivars(State {
            controller: controller.clone(),
            clock: RefCell::new(Weak::new()),
            ticks: Cell::new(0),
        });
        let target: Retained<ClockTarget> = unsafe { msg_send![super(allocated), init] };
        let screen = NSScreen::mainScreen(mtm).expect("probe display");
        let clock = Rc::new(unsafe { DisplayClock::new(&screen, &target) });
        *target.ivars().clock.borrow_mut() = Rc::downgrade(&clock);
        clock.set_maximum_fps(screen.maximumFramesPerSecond() as u32);
        clock.set_active(true, &controller);
        NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.3));
        let active_ticks = target.ivars().ticks.get();
        assert!(
            active_ticks > 0,
            "display link never woke its native target"
        );
        clock.set_active(false, &controller);
        NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.1));
        assert_eq!(
            target.ivars().ticks.get(),
            active_ticks,
            "paused display link continued waking"
        );
        drop(clock);
        controller.stop();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !controller.is_stopped() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(controller.is_stopped());
        println!("displayLinkCallbacks={active_ticks} pausedCallbacks=0 workerStopped=true");
    }
}

#[cfg(target_os = "macos")]
fn main() {
    probe::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("display_clock probe requires macOS");
}

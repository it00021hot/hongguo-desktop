#[cfg(target_os = "macos")]
#[path = "display_clock/frames.rs"]
mod frames;

#[cfg(target_os = "macos")]
mod probe {
    use hongguo_nsh_video_api::{Source, VideoProps, frames::register_frames};
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
        let mut callbacks = 0;
        for round in 0..10 {
            callbacks += switch_sources(mtm, round);
        }
        println!(
            "sourceSwitches=60 displayLinkRebuilds=10 displayLinkCallbacks={callbacks} pausedCallbacks=0 workersStopped=10"
        );
    }

    fn switch_sources(mtm: MainThreadMarker, round: u32) -> u32 {
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
        let mut registrations = Vec::new();
        for (index, fps) in [24, 30, 60, 24, 60, 30].into_iter().enumerate() {
            let id = format!("clock-switch-{round}-{index}");
            let registration = register_frames(&id, Arc::new(super::frames::Frames::new(fps)))
                .expect("register synthetic native frames");
            let mut source = Source::default();
            source.id = id;
            source.url = "http://127.0.0.1/unused-synthetic-clock".into();
            source.frame_source = Some(registration.url().into());
            let mut props = VideoProps::default();
            props.source = Some(source);
            props.active = true;
            props.paused = false;
            controller
                .set_props(props)
                .expect("switch native frame source");
            registrations.push(registration);
            let deadline = Instant::now() + Duration::from_secs(3);
            while controller.preferred_frame_rate() != fps && Instant::now() < deadline {
                pump(0.01);
            }
            assert_eq!(
                controller.preferred_frame_rate(),
                fps,
                "new source never supplied native frames"
            );
            // The first callback after native metadata arrives used to abort on macOS.
            let before = target.ivars().ticks.get();
            pump(0.1);
            assert!(
                target.ivars().ticks.get() > before,
                "display link stopped after source switch"
            );
        }
        controller
            .set_props(VideoProps::default())
            .expect("clear native frame source");
        let deadline = Instant::now() + Duration::from_secs(3);
        while controller.preferred_frame_rate() != 0 && Instant::now() < deadline {
            pump(0.01);
        }
        assert_eq!(controller.preferred_frame_rate(), 0);
        pump(0.1);
        let active_ticks = target.ivars().ticks.get();
        assert!(
            active_ticks > 0,
            "display link never woke its native target"
        );
        clock.set_active(false, &controller);
        pump(0.1);
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
        active_ticks
    }

    fn pump(seconds: f64) {
        NSRunLoop::mainRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(seconds));
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

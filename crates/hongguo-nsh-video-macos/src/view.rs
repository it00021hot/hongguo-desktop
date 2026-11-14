use hongguo_nsh_video_api::VideoEvent;
use hongguo_nsh_video_apple::{DisplayClock, OutputError, PlayerController};
use nsh_platform_macos::{
    objc2_app_kit::NSView,
    objc2_foundation::{NSPoint, NSRect, NSSize},
};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_quartz_core::{CALayer, CAMetalLayer};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
pub(super) struct ViewState {
    controller: RefCell<Option<Rc<PlayerController>>>,
    display_clock: RefCell<Option<DisplayClock>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ViewState]
    pub(super) struct PixelView;

    impl PixelView {
        #[unsafe(method(hongguoFrameTick:))]
        fn frame_tick(&self, _: &objc2::runtime::AnyObject) {
            if let (Some(clock), Some(controller)) = (self.ivars().display_clock.borrow().as_ref(), self.ivars().controller.borrow().as_ref()) {
                self.refresh_display(controller);
                clock.tick(controller);
            }
        }
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }

        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _: NSPoint) -> *mut NSView { std::ptr::null_mut() }

        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            // SAFETY: Forward this resize to NSView before reporting the resulting backing size.
            unsafe { let _: () = msg_send![super(self), setFrameSize: size]; }
            self.resize();
        }

        #[unsafe(method(viewDidChangeBackingProperties))]
        fn backing_changed(&self) { self.resize(); }

        #[unsafe(method(viewDidMoveToWindow))]
        fn moved_to_window(&self) { self.resize(); }
    }
);

impl PixelView {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let allocated = Self::alloc(mtm).set_ivars(ViewState::default());
        // SAFETY: Initialize this allocated NSView subclass with an empty frame.
        unsafe { msg_send![super(allocated), initWithFrame: NSRect::ZERO] }
    }

    pub(super) fn attach(
        &self,
        emit: impl Fn(VideoEvent) + Send + Sync + 'static,
    ) -> Result<Rc<PlayerController>, OutputError> {
        let root = CALayer::new();
        let metal = CAMetalLayer::new();
        self.setWantsLayer(true);
        self.setLayer(Some(&root));
        root.addSublayer(&metal);
        let controller = Rc::new(PlayerController::spawn(metal, emit)?);
        self.ivars().controller.replace(Some(controller.clone()));
        self.ivars()
            .display_clock
            .replace(Some(unsafe { DisplayClock::new(self, self) }));
        self.resize();
        Ok(controller)
    }

    fn resize(&self) {
        let Some(controller) = self.ivars().controller.borrow().clone() else {
            return;
        };
        let bounds = self.bounds();
        self.refresh_display(&controller);
        let pixels = self.convertSizeToBacking(bounds.size);
        let width = pixels.width.max(0.0).round() as u32;
        let height = pixels.height.max(0.0).round() as u32;
        let scale = if bounds.size.width > 0.0 {
            pixels.width / bounds.size.width
        } else {
            1.0
        };
        if let Err(error) =
            controller.resize(width, height, bounds.size.width, bounds.size.height, scale)
        {
            log::error!("native pixel video resize failed: {error}");
        }
    }

    fn refresh_display(&self, controller: &PlayerController) {
        // EDR headroom also changes with display brightness while playing;
        // a backing-size notification alone does not cover those changes.
        let screen = self.window().and_then(|window| window.screen());
        if let Some(screen) = &screen
            && let Some(clock) = self.ivars().display_clock.borrow().as_ref()
        {
            let maximum: isize = unsafe { msg_send![screen, maximumFramesPerSecond] };
            clock.set_maximum_fps(maximum.max(1) as u32);
        }
        let headroom = screen.map_or(1.0_f64, |screen| unsafe {
            msg_send![&screen, maximumExtendedDynamicRangeColorComponentValue]
        });
        if let Err(error) = controller.set_display_headroom(headroom.max(1.0) as f32) {
            log::error!("native pixel video display change failed: {error}");
        }
    }

    pub(super) fn set_clock_active(&self, active: bool) {
        if let (Some(clock), Some(controller)) = (
            self.ivars().display_clock.borrow().as_ref(),
            self.ivars().controller.borrow().as_ref(),
        ) {
            clock.set_active(active, controller);
        }
    }

    pub(super) fn detach_clock(&self) {
        self.ivars().display_clock.take();
    }
}

use hongguo_nsh_video_api::VideoEvent;
use hongguo_nsh_video_apple::{DisplayClock, OutputError, PlayerController};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_core_foundation::CGRect;
use objc2_quartz_core::CAMetalLayer;
use objc2_ui_kit::UIView;
use std::{cell::RefCell, sync::Arc};

#[derive(Default)]
pub(super) struct ViewState {
    controller: RefCell<Option<Arc<PlayerController>>>,
    display_clock: RefCell<Option<DisplayClock>>,
}

define_class!(
    #[unsafe(super(UIView))]
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
        #[unsafe(method(layoutSubviews))]
        fn layout_subviews(&self) {
            // SAFETY: Preserve UIView's layout before publishing this view's final bounds.
            unsafe { let _: () = msg_send![super(self), layoutSubviews]; }
            self.resize();
        }

        #[unsafe(method(didMoveToWindow))]
        fn moved_to_window(&self) {
            // SAFETY: This is the UIView lifecycle method of the current object.
            unsafe { let _: () = msg_send![super(self), didMoveToWindow]; }
            self.resize();
        }
    }
);

impl PixelView {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let allocated = Self::alloc(mtm).set_ivars(ViewState::default());
        // SAFETY: Initialize this allocated UIView subclass with an empty frame.
        let view: Retained<Self> =
            unsafe { msg_send![super(allocated), initWithFrame: CGRect::ZERO] };
        // NSH's parent owns touch handling and accessibility; pixels never swallow its gestures.
        view.setUserInteractionEnabled(false);
        view
    }

    pub(super) fn attach(
        &self,
        emit: impl Fn(VideoEvent) + Send + Sync + 'static,
    ) -> Result<Arc<PlayerController>, OutputError> {
        let metal = CAMetalLayer::new();
        self.layer().addSublayer(&metal);
        let controller = Arc::new(PlayerController::spawn(metal, emit)?);
        self.ivars().controller.replace(Some(controller.clone()));
        let screen: Retained<objc2::runtime::AnyObject> =
            unsafe { msg_send![objc2::class!(UIScreen), mainScreen] };
        self.ivars()
            .display_clock
            .replace(Some(unsafe { DisplayClock::new(&screen, self) }));
        self.resize();
        Ok(controller)
    }

    fn resize(&self) {
        let Some(controller) = self.ivars().controller.borrow().clone() else {
            return;
        };
        let bounds = self.bounds();
        self.refresh_display(&controller);
        let scale = self.contentScaleFactor();
        let width = (bounds.size.width * scale).max(0.0).round() as u32;
        let height = (bounds.size.height * scale).max(0.0).round() as u32;
        if let Err(error) =
            controller.resize(width, height, bounds.size.width, bounds.size.height, scale)
        {
            log::error!("native pixel video resize failed: {error}");
        }
    }

    fn refresh_display(&self, controller: &PlayerController) {
        let headroom = self.window().map_or(1.0_f64, |window| unsafe {
            let screen: objc2::rc::Retained<objc2::runtime::AnyObject> = msg_send![&window, screen];
            if let Some(clock) = self.ivars().display_clock.borrow().as_ref() {
                let maximum: isize = msg_send![&screen, maximumFramesPerSecond];
                clock.set_maximum_fps(maximum.max(1) as u32);
            }
            msg_send![&screen, currentEDRHeadroom]
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

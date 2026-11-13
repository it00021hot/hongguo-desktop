use hongguo_nsh_video_api::VideoEvent;
use hongguo_nsh_video_apple::{OriginalLayer, OutputError, PlayerController};
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
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ViewState]
    pub(super) struct PixelView;

    impl PixelView {
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
        let original = OriginalLayer::new()?;
        let metal = CAMetalLayer::new();
        self.setWantsLayer(true);
        self.setLayer(Some(&root));
        root.addSublayer(&original.layer());
        root.addSublayer(&metal);
        let controller = Rc::new(PlayerController::spawn(original, metal, emit)?);
        self.ivars().controller.replace(Some(controller.clone()));
        self.resize();
        Ok(controller)
    }

    fn resize(&self) {
        let Some(controller) = self.ivars().controller.borrow().clone() else {
            return;
        };
        let bounds = self.bounds();
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
}

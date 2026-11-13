use hongguo_nsh_video_api::VideoEvent;
use hongguo_nsh_video_apple::{OriginalLayer, OutputError, PlayerController};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_core_foundation::CGRect;
use objc2_quartz_core::CAMetalLayer;
use objc2_ui_kit::UIView;
use std::{cell::RefCell, sync::Arc};

#[derive(Default)]
pub(super) struct ViewState {
    controller: RefCell<Option<Arc<PlayerController>>>,
}

define_class!(
    #[unsafe(super(UIView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ViewState]
    pub(super) struct PixelView;

    impl PixelView {
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
        let original = OriginalLayer::new()?;
        let metal = CAMetalLayer::new();
        self.layer().addSublayer(&original.layer());
        self.layer().addSublayer(&metal);
        let controller = Arc::new(PlayerController::spawn(original, metal, emit)?);
        self.ivars().controller.replace(Some(controller.clone()));
        self.resize();
        Ok(controller)
    }

    fn resize(&self) {
        let Some(controller) = self.ivars().controller.borrow().clone() else {
            return;
        };
        let bounds = self.bounds();
        let scale = self.contentScaleFactor();
        let width = (bounds.size.width * scale).max(0.0).round() as u32;
        let height = (bounds.size.height * scale).max(0.0).round() as u32;
        if let Err(error) =
            controller.resize(width, height, bounds.size.width, bounds.size.height, scale)
        {
            log::error!("native pixel video resize failed: {error}");
        }
    }
}

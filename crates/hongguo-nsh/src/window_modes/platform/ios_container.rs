use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_foundation::NSObjectProtocol;
use objc2_ui_kit::{
    UIInterfaceOrientationMask, UIView, UIViewAutoresizing, UIViewController, UIWindow,
};
use std::cell::Cell;

pub(super) struct Ivars {
    pub original: Retained<UIViewController>,
    pub mask: Cell<UIInterfaceOrientationMask>,
    pub normal: UIInterfaceOrientationMask,
    pub content: Retained<UIView>,
}
define_class!(
    #[unsafe(super(UIViewController))]
    #[thread_kind=MainThreadOnly]
    #[ivars=Ivars]
    pub(super) struct OrientationContainer;

    unsafe impl NSObjectProtocol for OrientationContainer {}
    impl OrientationContainer {
        #[unsafe(method(supportedInterfaceOrientations))]
        fn supported_orientations(&self)->UIInterfaceOrientationMask {self.ivars().mask.get()}
        #[unsafe(method_id(childViewControllerForStatusBarStyle))]
        fn status_style_child(&self)->Option<Retained<UIViewController>> {Some(self.ivars().original.clone())}
        #[unsafe(method_id(childViewControllerForStatusBarHidden))]
        fn status_hidden_child(&self)->Option<Retained<UIViewController>> {Some(self.ivars().original.clone())}
        #[unsafe(method_id(childViewControllerForHomeIndicatorAutoHidden))]
        fn home_child(&self)->Option<Retained<UIViewController>> {Some(self.ivars().original.clone())}
    }
);

impl OrientationContainer {
    pub fn install(window: &UIWindow, mtm: MainThreadMarker) -> Result<Retained<Self>, String> {
        let original = window
            .rootViewController()
            .ok_or_else(|| "window has no root controller".to_owned())?;
        let normal = original.supportedInterfaceOrientations();
        original.loadViewIfNeeded();
        let content = original
            .view()
            .ok_or_else(|| "root controller has no content view".to_owned())?;
        let this = Self::alloc(mtm).set_ivars(Ivars {
            original: original.clone(),
            mask: Cell::new(normal),
            normal,
            content: content.clone(),
        });
        let controller: Retained<Self> = unsafe { msg_send![super(this), init] };
        let container = UIView::new(mtm);
        container.setFrame(window.bounds());
        controller.setView(Some(&container));
        window.setRootViewController(Some(&controller));
        controller.addChildViewController(&original);
        content.setFrame(container.bounds());
        content.setAutoresizingMask(
            UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight,
        );
        container.addSubview(&content);
        original.didMoveToParentViewController(Some(&controller));
        Ok(controller)
    }
    pub fn landscape(&self, enabled: bool) {
        self.ivars().mask.set(if enabled {
            UIInterfaceOrientationMask::Landscape
        } else {
            self.ivars().normal
        });
        self.setNeedsUpdateOfSupportedInterfaceOrientations();
    }
    pub fn restore(&self, window: &UIWindow) {
        self.landscape(false);
        let original = &self.ivars().original;
        original.willMoveToParentViewController(None);
        self.ivars().content.removeFromSuperview();
        original.removeFromParentViewController();
        window.setRootViewController(Some(original));
    }
}

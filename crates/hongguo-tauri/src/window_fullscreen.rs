use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use hongguo_common::{AppError, AppResult};
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSWindow, NSWindowDidExitFullScreenNotification, NSWindowWillExitFullScreenNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter};
use tauri::{AppHandle, Emitter, Manager};

const EXITED_EVENT: &str = "window-fullscreen-exited";
static EXITING: AtomicBool = AtomicBool::new(false);

struct ObserverState {
    app: AppHandle,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ObserverState]
    struct FullscreenObserver;

    impl FullscreenObserver {
        #[unsafe(method(windowWillExitFullScreen:))]
        fn will_exit(&self, _: &NSNotification) {
            EXITING.store(true, Ordering::Release);
        }

        #[unsafe(method(windowDidExitFullScreen:))]
        fn did_exit(&self, _: &NSNotification) {
            EXITING.store(false, Ordering::Release);
            if let Err(error) = self.ivars().app.emit_to("main", EXITED_EVENT, ()) {
                log::error!("[Window] 全屏退出事件发送失败: {error}");
            }
        }
    }
);

impl Drop for FullscreenObserver {
    fn drop(&mut self) {
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(self) };
    }
}

thread_local! {
    static OBSERVER: RefCell<Option<Retained<FullscreenObserver>>> = const { RefCell::new(None) };
}

pub(crate) fn init(app: &AppHandle) -> AppResult<()> {
    let main_thread = MainThreadMarker::new()
        .ok_or_else(|| AppError::Internal("fullscreen observer requires main thread".into()))?;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::NotFound("main window".into()))?;
    let pointer = window
        .ns_window()
        .map_err(|error| AppError::Internal(format!("observe fullscreen: {error}")))?;
    let native_window = unsafe { &*(pointer as *const NSWindow) };
    let observer =
        FullscreenObserver::alloc(main_thread).set_ivars(ObserverState { app: app.clone() });
    let observer: Retained<FullscreenObserver> = unsafe { msg_send![super(observer), init] };
    let center = NSNotificationCenter::defaultCenter();
    unsafe {
        center.addObserver_selector_name_object(
            &observer,
            sel!(windowWillExitFullScreen:),
            Some(NSWindowWillExitFullScreenNotification),
            Some(native_window),
        );
        center.addObserver_selector_name_object(
            &observer,
            sel!(windowDidExitFullScreen:),
            Some(NSWindowDidExitFullScreenNotification),
            Some(native_window),
        );
    }
    OBSERVER.with(|slot| *slot.borrow_mut() = Some(observer));
    Ok(())
}

pub(crate) fn is_exiting() -> bool {
    EXITING.load(Ordering::Acquire)
}

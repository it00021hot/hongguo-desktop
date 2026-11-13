#[path = "ios_container.rs"]
mod container;
use super::super::wire::{MobileRequest, MobileResponse};
use container::OrientationContainer;
use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, rc::Retained};
use objc2_foundation::{NSError, NSRunLoop, NSRunLoopCommonModes, NSTimer};
use objc2_ui_kit::{
    UIInterfaceOrientation, UIInterfaceOrientationMask, UIView, UIWindow,
    UIWindowSceneGeometryPreferencesIOS,
};
use serde_json::Value;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(3);
const TICK_SECONDS: f64 = 0.04;
type Errors = Arc<Mutex<Vec<(u32, String)>>>;

struct Pending {
    request: MobileRequest,
    mask: UIInterfaceOrientationMask,
    deadline: Instant,
}
struct Orientation {
    view: Retained<UIView>,
    container: Option<Retained<OrientationContainer>>,
    context: ComponentContext,
    serial: u32,
    active: bool,
    saved: Option<UIInterfaceOrientationMask>,
    pending: Option<Pending>,
    errors: Errors,
}

impl Orientation {
    fn window(&self) -> Result<Retained<UIWindow>, String> {
        self.view
            .window()
            .ok_or_else(|| "native window is not attached".into())
    }
    fn attach(&mut self) -> Result<(), String> {
        if self.container.is_none() {
            let window = self.window()?;
            self.container = Some(OrientationContainer::install(&window, self.view.mtm())?);
        }
        Ok(())
    }
    fn request(&mut self, request: MobileRequest) {
        self.serial = request.serial;
        if let Err(error) = self.begin(request) {
            self.respond(request.serial, Some(error));
        }
    }
    fn begin(&mut self, request: MobileRequest) -> Result<(), String> {
        self.attach()?;
        let scene = self
            .window()?
            .windowScene()
            .ok_or_else(|| "window has no scene".to_owned())?;
        if request.landscape && self.saved.is_none() {
            let current = mask(scene.effectiveGeometry().interfaceOrientation());
            if current.is_empty() {
                return Err("scene orientation is not available".into());
            }
            self.saved = Some(current);
        }
        let target = if request.landscape {
            UIInterfaceOrientationMask::Landscape
        } else if let Some(saved) = self.saved {
            saved
        } else {
            self.active = false;
            self.respond(request.serial, None);
            return Ok(());
        };
        self.container
            .as_ref()
            .expect("attached container")
            .landscape(request.landscape);
        let preferences = UIWindowSceneGeometryPreferencesIOS::initWithInterfaceOrientations(
            UIWindowSceneGeometryPreferencesIOS::alloc(),
            target,
        );
        let errors = self.errors.clone();
        let serial = request.serial;
        let error_handler = block2::RcBlock::new(move |error: std::ptr::NonNull<NSError>| {
            errors.lock().expect("orientation callback queue").push((
                serial,
                unsafe { error.as_ref() }.localizedDescription().to_string(),
            ));
        });
        self.pending = Some(Pending {
            request,
            mask: target,
            deadline: Instant::now() + CONFIRMATION_TIMEOUT,
        });
        scene.requestGeometryUpdateWithPreferences_errorHandler(&preferences, Some(&error_handler));
        self.tick();
        Ok(())
    }
    fn tick(&mut self) {
        let _ = self.attach();
        let errors = std::mem::take(&mut *self.errors.lock().expect("orientation callback queue"));
        for (serial, error) in errors {
            if self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.request.serial == serial)
            {
                self.pending = None;
                if let Some(container) = &self.container {
                    container.landscape(self.active);
                }
                if !self.active {
                    self.saved = None;
                }
                self.respond(serial, Some(error));
            }
        }
        let actual = self
            .window()
            .ok()
            .and_then(|window| window.windowScene())
            .map(|scene| mask(scene.effectiveGeometry().interfaceOrientation()));
        let Some(pending) = &self.pending else {
            if self.active
                && actual.is_some_and(|actual| {
                    !actual.is_empty() && !actual.intersects(UIInterfaceOrientationMask::Landscape)
                })
            {
                self.active = false;
                self.saved = None;
                if let Some(container) = &self.container {
                    container.landscape(false);
                }
                self.respond(
                    self.serial,
                    Some("The system changed the requested screen orientation".into()),
                );
            }
            return;
        };
        if actual.is_some_and(|actual| pending.mask.intersects(actual)) {
            let pending = self.pending.take().expect("pending orientation");
            self.active = pending.request.landscape;
            if !self.active {
                self.saved = None;
            }
            self.respond(pending.request.serial, None);
        } else if Instant::now() >= pending.deadline {
            let pending = self.pending.take().expect("pending orientation");
            if let Some(container) = &self.container {
                container.landscape(self.active);
            }
            if !self.active {
                self.saved = None;
            }
            self.respond(
                pending.request.serial,
                Some("The system did not confirm the requested screen orientation".into()),
            );
        }
    }
    fn respond(&self, serial: u32, error: Option<String>) {
        if let Some(error) = &error {
            log::warn!("[NSH orientation] request failed: {error}");
        }
        self.context.emit(
            "orientation",
            serde_json::to_value(MobileResponse {
                serial,
                landscape: self.active,
                error,
            })
            .expect("orientation response"),
        );
    }
}

fn mask(orientation: UIInterfaceOrientation) -> UIInterfaceOrientationMask {
    match orientation {
        UIInterfaceOrientation::LandscapeLeft => UIInterfaceOrientationMask::LandscapeLeft,
        UIInterfaceOrientation::LandscapeRight => UIInterfaceOrientationMask::LandscapeRight,
        UIInterfaceOrientation::PortraitUpsideDown => {
            UIInterfaceOrientationMask::PortraitUpsideDown
        }
        UIInterfaceOrientation::Portrait => UIInterfaceOrientationMask::Portrait,
        _ => UIInterfaceOrientationMask::empty(),
    }
}

struct Host {
    view: Retained<UIView>,
    state: Rc<RefCell<Orientation>>,
    timer: Retained<NSTimer>,
}
impl NativeComponent for Host {
    fn view(&self) -> &UIView {
        &self.view
    }
    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name != "request" {
            return;
        }
        if let Some(value) = value.filter(|value| !value.is_null()) {
            match serde_json::from_value::<MobileRequest>(value.clone()) {
                Ok(request) => self.state.borrow_mut().request(request),
                Err(error) => self
                    .state
                    .borrow()
                    .context
                    .emit("failure", Value::String(error.to_string())),
            }
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.timer.invalidate();
        let state = self.state.borrow();
        if let (Some(container), Ok(window)) = (&state.container, state.window()) {
            container.restore(&window);
            if let (Some(saved), Some(scene)) = (state.saved, window.windowScene()) {
                let preferences =
                    UIWindowSceneGeometryPreferencesIOS::initWithInterfaceOrientations(
                        UIWindowSceneGeometryPreferencesIOS::alloc(),
                        saved,
                    );
                let error_handler = block2::RcBlock::new(|error: std::ptr::NonNull<NSError>| {
                    log::warn!(
                        "[NSH orientation] cleanup failed: {}",
                        unsafe { error.as_ref() }.localizedDescription()
                    );
                });
                scene.requestGeometryUpdateWithPreferences_errorHandler(
                    &preferences,
                    Some(&error_handler),
                );
            }
        }
    }
}

pub(crate) fn register(components: &Components) {
    components.register(
        "hongguo.window-modes",
        Rc::new(|context: ComponentContext| {
            let view = UIView::new(MainThreadMarker::new().expect("native component main thread"));
            let state = Rc::new(RefCell::new(Orientation {
                view: view.clone(),
                container: None,
                context,
                serial: 0,
                active: false,
                saved: None,
                pending: None,
                errors: Arc::default(),
            }));
            let weak = Rc::downgrade(&state);
            let block = block2::RcBlock::new(move |_| {
                if let Some(state) = weak.upgrade() {
                    state.borrow_mut().tick();
                }
            });
            let timer =
                unsafe { NSTimer::timerWithTimeInterval_repeats_block(TICK_SECONDS, true, &block) };
            unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
            Box::new(Host { view, state, timer })
        }),
    );
}

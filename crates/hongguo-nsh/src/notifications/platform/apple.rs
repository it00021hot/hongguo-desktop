use super::super::wire::{Action, Request, Response};
use block2::RcBlock;
#[cfg(target_os = "ios")]
use nsh_platform_ios as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos as platform;
#[cfg(target_os = "macos")]
use nsh_platform_macos::{
    objc2::{
        self, AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send, rc::Retained,
        runtime::ProtocolObject,
    },
    objc2_app_kit::NSView as View,
    objc2_foundation::{NSBundle, NSError, NSObject, NSObjectProtocol, NSString},
};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use platform::{ComponentContext, Components, NativeComponent};
use serde_json::Value;
use std::rc::Rc;
#[cfg(target_os = "ios")]
use {
    objc2::{
        self, AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send, rc::Retained,
        runtime::ProtocolObject,
    },
    objc2_foundation::{NSBundle, NSError, NSObject, NSObjectProtocol, NSString},
    objc2_ui_kit::UIView as View,
};

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars=ComponentContext]
    struct NotificationDelegate;

    unsafe impl NSObjectProtocol for NotificationDelegate {}
    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion
                .call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List,));
        }
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn receive(
            &self,
            _center: &UNUserNotificationCenter,
            _response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            self.ivars().emit("activated", Value::Null);
            completion.call(());
        }
    }
);

struct Host {
    view: Retained<View>,
    context: ComponentContext,
    serial: u32,
    delegate: Retained<NotificationDelegate>,
    center: Option<Retained<UNUserNotificationCenter>>,
    previous: Option<Retained<ProtocolObject<dyn UNUserNotificationCenterDelegate>>>,
}

impl NativeComponent for Host {
    #[cfg(target_os = "ios")]
    fn view(&self) -> &View {
        &self.view
    }
    #[cfg(target_os = "macos")]
    fn view(&self) -> Retained<View> {
        self.view.clone()
    }

    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name != "requests" {
            return;
        }
        let Some(value) = value else {
            return;
        };
        match serde_json::from_value::<Vec<Request>>(value.clone()) {
            Ok(requests) => {
                for request in requests {
                    if request.serial > self.serial {
                        self.serial = request.serial;
                        match self.center() {
                            Ok(center) => dispatch(&center, self.context.clone(), request),
                            Err(error) => emit(&self.context, request.serial, false, Some(error)),
                        }
                    }
                }
            }
            Err(error) => log::error!("[NSH notifications] invalid native request: {error}"),
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let Some(center) = &self.center else {
            return;
        };
        if center
            .delegate()
            .is_some_and(|delegate| delegate.isEqual(Some(&*self.delegate)))
        {
            center.setDelegate(self.previous.as_deref());
        }
    }
}

impl Host {
    fn center(&mut self) -> Result<Retained<UNUserNotificationCenter>, String> {
        if let Some(center) = &self.center {
            return Ok(center.clone());
        }
        let bundle = NSBundle::mainBundle();
        if bundle.bundleIdentifier().is_none() || !bundle.bundlePath().to_string().ends_with(".app")
        {
            return Err("System notifications require the installed Hongguo .app bundle".into());
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        self.previous = center.delegate();
        center.setDelegate(Some(ProtocolObject::from_ref(&*self.delegate)));
        self.center = Some(center.clone());
        Ok(center)
    }
}

fn dispatch(center: &UNUserNotificationCenter, context: ComponentContext, request: Request) {
    let serial = request.serial;
    match request.action {
        Action::RequestPermission => {
            let callback =
                RcBlock::new(move |granted: objc2::runtime::Bool, error: *mut NSError| {
                    emit(&context, serial, granted.as_bool(), error_text(error));
                });
            center.requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert,
                &callback,
            );
        }
        Action::Send { id, title, body } => {
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&title));
            content.setBody(&NSString::from_str(&body));
            let notification = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &NSString::from_str(&id),
                &content,
                None,
            );
            let callback = RcBlock::new(move |error: *mut NSError| {
                emit(&context, serial, error.is_null(), error_text(error));
            });
            center.addNotificationRequest_withCompletionHandler(&notification, Some(&callback));
        }
    }
}

fn error_text(error: *mut NSError) -> Option<String> {
    unsafe { error.as_ref() }.map(|error| error.localizedDescription().to_string())
}

fn emit(context: &ComponentContext, serial: u32, granted: bool, error: Option<String>) {
    context.emit(
        "result",
        serde_json::to_value(Response {
            serial,
            granted,
            error,
        })
        .expect("notification response serialization"),
    );
}

pub(crate) fn register(components: &Components) {
    components.register(
        "hongguo.notifications",
        Rc::new(|context| {
            let mtm = MainThreadMarker::new().expect("native component main thread");
            let delegate = NotificationDelegate::alloc().set_ivars(context.clone());
            let delegate: Retained<NotificationDelegate> =
                unsafe { msg_send![super(delegate), init] };
            Box::new(Host {
                view: View::new(mtm),
                context,
                serial: 0,
                delegate,
                center: None,
                previous: None,
            })
        }),
    );
}

use super::super::wire::{Action, Request, Response};
use nsh_platform_windows::{ComponentContext, Components, NativeComponent};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};
use windows::{
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{
        NotificationSetting, ToastDismissedEventArgs, ToastFailedEventArgs, ToastNotification,
        ToastNotificationManager,
    },
    Win32::System::{
        Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegSetValueExW,
        },
        WinRT::{RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize},
    },
    core::{HSTRING, IInspectable, PCWSTR, h, w},
};

const APP_ID: &str = "com.hongguo.nsh";

struct Host {
    context: ComponentContext,
    serial: u32,
    initialized: windows::core::Result<()>,
    active: Arc<Mutex<BTreeMap<u32, ToastNotification>>>,
}

impl NativeComponent for Host {
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
                    if request.serial <= self.serial {
                        continue;
                    }
                    self.serial = request.serial;
                    let result = self
                        .initialized
                        .clone()
                        .and_then(|()| self.dispatch(request.serial, request.action));
                    let (granted, error) = match result {
                        Ok(granted) => (granted, None),
                        Err(error) => (false, Some(error.to_string())),
                    };
                    self.context.emit(
                        "result",
                        serde_json::to_value(Response {
                            serial: request.serial,
                            granted,
                            error,
                        })
                        .expect("notification response serialization"),
                    );
                }
            }
            Err(error) => log::error!("[NSH notifications] invalid native request: {error}"),
        }
    }
}

impl Host {
    fn dispatch(&self, serial: u32, action: Action) -> windows::core::Result<bool> {
        register_identity()?;
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&APP_ID.into())?;
        if notifier.Setting()? != NotificationSetting::Enabled {
            return Ok(false);
        }
        let Action::Send { id, title, body } = action else {
            return Ok(true);
        };
        let document = document(&title, &body)?;
        let toast = ToastNotification::CreateToastNotification(&document)?;
        // Follow notice IDs are bounded by Windows' 64-character toast tag limit.
        toast.SetTag(&HSTRING::from(&id))?;
        let context = self.context.clone();
        let active = Arc::downgrade(&self.active);
        toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
            move |_, _| {
                context.emit("activated", Value::Null);
                if let Some(active) = active.upgrade() {
                    active.lock().expect("notification state").remove(&serial);
                }
                Ok(())
            },
        ))?;
        let active = Arc::downgrade(&self.active);
        toast.Dismissed(&TypedEventHandler::<
            ToastNotification,
            ToastDismissedEventArgs,
        >::new(move |_, _| {
            if let Some(active) = active.upgrade() {
                active.lock().expect("notification state").remove(&serial);
            }
            Ok(())
        }))?;
        let active = Arc::downgrade(&self.active);
        toast.Failed(
            &TypedEventHandler::<ToastNotification, ToastFailedEventArgs>::new(move |_, error| {
                if let Some(error) = error.as_ref() {
                    log::warn!(
                        "[NSH notifications] Windows delivery failed: {:?}",
                        error.ErrorCode()?
                    );
                }
                if let Some(active) = active.upgrade() {
                    active.lock().expect("notification state").remove(&serial);
                }
                Ok(())
            }),
        )?;
        self.active
            .lock()
            .expect("notification state")
            .insert(serial, toast.clone());
        if let Err(error) = notifier.Show(&toast) {
            self.active
                .lock()
                .expect("notification state")
                .remove(&serial);
            return Err(error);
        }
        Ok(true)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.active.lock().expect("notification state").clear();
        if self.initialized.is_ok() {
            unsafe { RoUninitialize() };
        }
    }
}

fn document(title: &str, body: &str) -> windows::core::Result<XmlDocument> {
    let document = XmlDocument::new()?;
    let toast = document.CreateElement(h!("toast"))?;
    document.AppendChild(&toast)?;
    let visual = document.CreateElement(h!("visual"))?;
    toast.AppendChild(&visual)?;
    let binding = document.CreateElement(h!("binding"))?;
    binding.SetAttribute(h!("template"), h!("ToastGeneric"))?;
    visual.AppendChild(&binding)?;
    for value in [title, body] {
        let text = document.CreateElement(h!("text"))?;
        text.SetInnerText(&value.into())?;
        binding.AppendChild(&text)?;
    }
    let audio = document.CreateElement(h!("audio"))?;
    audio.SetAttribute(h!("silent"), h!("true"))?;
    toast.AppendChild(&audio)?;
    Ok(document)
}

fn register_identity() -> windows::core::Result<()> {
    let path: Vec<u16> = format!(r"Software\Classes\AppUserModelId\{APP_ID}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
    }
    let name: Vec<u8> = "红果\0".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let result = unsafe { RegSetValueExW(key, w!("DisplayName"), None, REG_SZ, Some(&name)).ok() };
    let closed = unsafe { RegCloseKey(key).ok() };
    result.and(closed)
}

pub(crate) fn register(components: &Components) {
    components.register(
        "hongguo.notifications",
        Rc::new(|context| {
            Box::new(Host {
                context,
                serial: 0,
                initialized: unsafe { RoInitialize(RO_INIT_SINGLETHREADED) },
                active: Arc::default(),
            })
        }),
    );
}

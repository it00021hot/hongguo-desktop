use super::{ExportRequest, ExportResponse};
use nsh_platform_ios::{ComponentContext, Components, NativeComponent};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject,
};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString, NSURL};
use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController, UIView};
use serde_json::Value;
use std::{cell::RefCell, rc::Rc};

struct Pending {
    serial: u32,
    context: ComponentContext,
}

pub(super) struct DelegateState {
    pending: RefCell<Option<Pending>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind=MainThreadOnly]
    #[ivars=DelegateState]
    struct ExportDelegate;

    unsafe impl NSObjectProtocol for ExportDelegate {}
    unsafe impl UIDocumentPickerDelegate for ExportDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn selected(&self, _picker: &UIDocumentPickerViewController, _urls: &NSArray<NSURL>) {
            self.finish(false, None);
        }
        #[unsafe(method(documentPickerWasCancelled:))]
        fn cancelled(&self, _picker: &UIDocumentPickerViewController) {
            self.finish(true, None);
        }
    }
);

impl ExportDelegate {
    fn finish(&self, is_cancelled: bool, error: Option<String>) {
        if let Some(pending) = self.ivars().pending.borrow_mut().take() {
            respond(&pending.context, pending.serial, is_cancelled, error);
        }
    }
}

struct Host {
    view: Retained<UIView>,
    context: ComponentContext,
    delegate: Retained<ExportDelegate>,
    picker: Option<Retained<UIDocumentPickerViewController>>,
}

impl Host {
    fn export(&mut self, request: ExportRequest) -> Result<(), String> {
        if self.delegate.ivars().pending.borrow().is_some() {
            return Err("A file export is already in progress".into());
        }
        let path = std::path::Path::new(&request.path);
        let metadata = path.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            return Err("The export source is not a file".into());
        }
        let window = self
            .view
            .window()
            .ok_or("The export view is not attached")?;
        let mut controller = window
            .rootViewController()
            .ok_or("The export window has no controller")?;
        while let Some(presented) = controller.presentedViewController() {
            controller = presented;
        }
        let url = NSURL::fileURLWithPath(&NSString::from_str(&request.path));
        let urls = NSArray::from_retained_slice(&[url]);
        let picker = UIDocumentPickerViewController::initForExportingURLs_asCopy(
            UIDocumentPickerViewController::alloc(self.view.mtm()),
            &urls,
            true,
        );
        picker.setDelegate(Some(ProtocolObject::from_ref(&*self.delegate)));
        *self.delegate.ivars().pending.borrow_mut() = Some(Pending {
            serial: request.serial,
            context: self.context.clone(),
        });
        controller.presentViewController_animated_completion(&picker, true, None);
        self.picker = Some(picker);
        Ok(())
    }
}

impl NativeComponent for Host {
    fn view(&self) -> &UIView {
        &self.view
    }
    fn set_prop(&mut self, name: &str, value: Option<&Value>) {
        if name != "request" {
            return;
        }
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return;
        };
        match serde_json::from_value::<ExportRequest>(value.clone()) {
            Ok(request) => {
                let serial = request.serial;
                if let Err(error) = self.export(request) {
                    respond(&self.context, serial, false, Some(error));
                }
            }
            Err(error) => log::error!("[NSH export] Invalid native request: {error}"),
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if self.delegate.ivars().pending.borrow().is_some() {
            if let Some(picker) = &self.picker {
                picker.dismissViewControllerAnimated_completion(false, None);
            }
            self.delegate.finish(true, None);
        }
    }
}

fn respond(context: &ComponentContext, serial: u32, is_cancelled: bool, error: Option<String>) {
    context.emit(
        "completed",
        serde_json::to_value(ExportResponse {
            serial,
            is_cancelled,
            error,
        })
        .expect("file export response"),
    );
}

pub(crate) fn register(components: &Components) {
    components.register(
        "hongguo.file-export",
        Rc::new(|context: ComponentContext| {
            let mtm = MainThreadMarker::new().expect("native export main thread");
            let delegate = ExportDelegate::alloc(mtm).set_ivars(DelegateState {
                pending: RefCell::new(None),
            });
            let delegate: Retained<ExportDelegate> = unsafe { msg_send![super(delegate), init] };
            Box::new(Host {
                view: UIView::new(mtm),
                context,
                delegate,
                picker: None,
            })
        }),
    );
}

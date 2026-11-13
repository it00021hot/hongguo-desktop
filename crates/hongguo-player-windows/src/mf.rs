//! MediaEngine 仅承载音轨与时钟，不配置系统视频解码设备。

use std::sync::OnceLock;
use windows::Win32::Media::MediaFoundation::{
    CLSID_MFMediaEngineClassFactory, IMFMediaEngine, IMFMediaEngineClassFactory, IMFMediaEngineEx,
    IMFMediaEngineNotify, MF_MEDIA_ENGINE_CALLBACK, MF_VERSION, MFCreateAttributes, MFStartup,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::{Interface, Result};

pub(super) fn startup() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        if let Err(error) = unsafe { MFStartup(MF_VERSION, 0) } {
            log::error!("[WmDirect] MFStartup 失败: {error}");
        }
    });
}

pub(super) fn create_engine(
    notify: &IMFMediaEngineNotify,
) -> Result<(IMFMediaEngine, IMFMediaEngineEx)> {
    let factory: IMFMediaEngineClassFactory =
        unsafe { CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_ALL)? };
    let mut store = None;
    unsafe {
        MFCreateAttributes(&mut store, 1)?;
        let store = store.expect("MFCreateAttributes 返回空属性表");
        store.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, notify)?;
        let engine = factory.CreateInstance(0, &store)?;
        let extended = engine.cast()?;
        Ok((engine, extended))
    }
}

pub(super) fn rate_supported(extended: &IMFMediaEngineEx, rate: f64) -> bool {
    unsafe { extended.IsPlaybackRateSupported(rate) }.as_bool()
}

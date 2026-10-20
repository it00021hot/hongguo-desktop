//! Media Foundation 引擎的薄封装：进程级初始化、引擎创建、倍速能力探测。
//!
//! 线程纪律（spike 实测得出，见 `docs/gpui-system-player-windows.md` §5）：
//! 引擎、属性表、`IMFByteStream`、视觉全在 **GPUI 主线程（STA）** 创建——DComp
//! 视觉来自渲染器，跨套间不保证可用；MediaEngine 的控制方法本身是自由线程的，
//! 壳侧的 `PlayerLike` 调用也都在主线程。`MFStartup` 与引擎同线程一次。

use std::sync::OnceLock;

use windows::Win32::Graphics::DirectComposition::IDCompositionVisual;
use windows::Win32::Media::MediaFoundation::{
    CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFMediaEngine, IMFMediaEngineClassFactory,
    IMFMediaEngineEx, IMFMediaEngineNotify, MF_MEDIA_ENGINE_CALLBACK,
    MF_MEDIA_ENGINE_PLAYBACK_VISUAL, MF_VERSION, MFCreateAttributes, MFStartup,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::{IUnknown, Interface, Result};

/// mfapi 版本号（与 hongguo-media-platform-windows 的 `MF_VERSION` 同值）。
const MF_API_VERSION: u32 = 0x0002_0070;

/// 进程级初始化：`MFStartup` 只做一次。主线程是 STA（gpui 调了 OleInitialize），
/// spike 已实测 STA 下引擎/供数/合成全通。
pub(super) fn startup() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        if let Err(error) = unsafe { MFStartup(MF_VERSION.max(MF_API_VERSION), 0) } {
            // 起不来就是系统后端整体不可用：壳侧随后 open 会失败并回滚 gpui。
            log::error!("[WmPresenter] MFStartup 失败: {error}");
        }
    });
}

/// 创建 MediaEngine（rendering mode：引擎自己把音视频渲染进我们给的视觉）。
///
/// 两个属性缺一不可：
/// - `MF_MEDIA_ENGINE_CALLBACK`：事件回调（首帧/缓冲/播完/失败）；
/// - `MF_MEDIA_ENGINE_PLAYBACK_VISUAL`：画面落点，**决定 rendering mode**。
///
/// **不要**给 `MF_MEDIA_ENGINE_DXGI_MANAGER`：引擎会拿渲染器那台 D3D11 设备
/// 去建视频处理链，而 GPUI 的设备没有 `D3D11_CREATE_DEVICE_VIDEO_SUPPORT`、
/// 也没开多线程保护——实测开播 60ms 内 `DXGI_ERROR_DEVICE_REMOVED`，整个窗口
/// 崩溃重建设备。引擎用自己那台设备、跨设备把内容交给合成器反而稳。
pub(super) fn create_engine(
    visual: &IDCompositionVisual,
    notify: &IMFMediaEngineNotify,
) -> Result<(IMFMediaEngine, IMFMediaEngineEx)> {
    let factory: IMFMediaEngineClassFactory =
        unsafe { CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_ALL)? };
    let mut store: Option<IMFAttributes> = None;
    unsafe { MFCreateAttributes(&mut store, 2)? };
    let store = store.expect("MFCreateAttributes 返回空属性表");

    let callback: IUnknown = notify.cast()?;
    let target: IUnknown = visual.cast()?;
    unsafe {
        store.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &callback)?;
        store.SetUnknown(&MF_MEDIA_ENGINE_PLAYBACK_VISUAL, &target)?;
    }

    let engine: IMFMediaEngine = unsafe { factory.CreateInstance(0, &store)? };
    let extended: IMFMediaEngineEx = engine.cast()?;
    Ok((engine, extended))
}

/// 这一档倍速引擎是否认（不认时调用方保持 1x 并记日志，别让 UI 与实际脱节）。
pub(super) fn rate_supported(extended: &IMFMediaEngineEx, rate: f64) -> bool {
    unsafe { extended.IsPlaybackRateSupported(rate) }.as_bool()
}

/// 引擎统计里的一个数值（MF 的 statistics 都是 UI4/UI8 族）。
pub(super) fn statistic(
    extended: &IMFMediaEngineEx,
    id: windows::Win32::Media::MediaFoundation::MF_MEDIA_ENGINE_STATISTIC,
) -> Option<u64> {
    let value = unsafe { extended.GetStatistics(id) }.ok()?;
    unsafe {
        let payload = &value.Anonymous.Anonymous;
        // VARENUM：2=I2 3=I4 4=UI4 11=BOOL 18=UI2 19=UI4 20=I8 21=UI8
        match payload.vt.0 {
            2 | 3 | 11 | 18 => Some(u64::from(payload.Anonymous.lVal as u32)),
            4 | 19 => Some(u64::from(payload.Anonymous.ulVal)),
            20 | 21 => Some(payload.Anonymous.uhVal),
            _ => None,
        }
    }
}

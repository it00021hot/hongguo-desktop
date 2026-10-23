//! Media Foundation 引擎的薄封装：进程级初始化、共享解码设备、引擎创建、倍速能力探测。
//!
//! 线程纪律（spike 实测得出，见 `docs/gpui-system-player-windows.md` §5）：
//! 引擎、属性表、`IMFByteStream` 全在 **GPUI 主线程（STA）** 创建；MediaEngine
//! 的控制方法本身是自由线程的，壳侧的 `PlayerLike` 调用也都在主线程。
//! `MFStartup` 与引擎同线程一次。
//!
//! **解码设备就是渲染器那台**（对齐 nsh-platform-windows 的 gfx 纪律）：渲染器
//! 设备在 vendor 补丁里补了 `VIDEO_SUPPORT` 与多线程保护后，frame-server 引擎
//! 经 `IMFDXGIDeviceManager` 直接在它上面解码，`TransferVideoFrame` 产出的
//! 纹理渲染器当场就能采样——零拷贝、零跨设备。历史教训：给 GPUI 设备但**不补
//! 旗标**，开播 60ms 内 `DXGI_ERROR_DEVICE_REMOVED`（Phase 2 坑 3）；让引擎
//! 每集自建设备，旧引擎 Shutdown 后开销 MF/驱动不归还（连播内存逐集上涨）。

use std::sync::{Mutex, OnceLock};

use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Media::MediaFoundation::{
    CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFDXGIDeviceManager, IMFMediaEngine,
    IMFMediaEngineClassFactory, IMFMediaEngineEx, IMFMediaEngineNotify, MF_MEDIA_ENGINE_CALLBACK,
    MF_MEDIA_ENGINE_DXGI_MANAGER, MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, MF_VERSION,
    MFCreateAttributes, MFCreateDXGIDeviceManager, MFStartup,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::{IUnknown, Interface, Result};

/// mfapi 版本号（与 hongguo-media-platform-windows 的 `MF_VERSION` 同值）。
const MF_API_VERSION: u32 = 0x0002_0070;

/// 进程级初始化：`MFStartup` 只做一次。主线程是 STA（gpui 调了 OleInitialize），
/// spike 已实测 STA 下引擎/供数全通。
pub(super) fn startup() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        if let Err(error) = unsafe { MFStartup(MF_VERSION.max(MF_API_VERSION), 0) } {
            // 起不来就是系统后端整体不可用：壳侧随后 open 会失败并走起播错误态。
            log::error!("[WmPresenter] MFStartup 失败: {error}");
        }
    });
}

/// 共享设备管理器的静态槽（接口本体非 `Send/Sync`；管理器是 MF 的自由线程
/// 对象，这里只在 GPUI 主线程创建与调用，跨线程只存在克隆）。
struct SharedManager {
    manager: Option<(IMFDXGIDeviceManager, u32)>,
    /// 设备管理器当前挂载的设备代际（渲染器设备丢失重建后要重挂）。
    held_generation: u64,
}
// SAFETY: 见类型注释；与 `SystemPlayer` 的 unsafe impl 同一套约定。
unsafe impl Send for SharedManager {}
unsafe impl Sync for SharedManager {}

/// 挂着渲染器设备的 DXGI 设备管理器（进程级一个，全部引擎共用）。
///
/// `generation` 是渲染器设备的代际（设备丢失重建后变化）：变了就
/// `ResetDevice` 到新设备，引擎下一帧起在新设备上解码。
pub(super) fn device_manager(
    device: &ID3D11Device,
    generation: u64,
) -> Option<IMFDXGIDeviceManager> {
    static SLOT: OnceLock<Mutex<SharedManager>> = OnceLock::new();
    let slot = SLOT.get_or_init(|| {
        Mutex::new(SharedManager {
            manager: None,
            held_generation: u64::MAX,
        })
    });
    let mut slot = slot.lock().expect("设备管理器锁 poisoned");
    if slot.held_generation == generation
        && let Some((manager, _token)) = &slot.manager
    {
        return Some(manager.clone());
    }
    let (manager, token) = match &slot.manager {
        Some((manager, token)) => (manager.clone(), *token),
        None => {
            let mut token = 0u32;
            let mut manager = None;
            if let Err(error) = unsafe { MFCreateDXGIDeviceManager(&mut token, &mut manager) } {
                log::error!("[WmPresenter] DXGI 设备管理器创建失败: {error}");
                return None;
            }
            let manager = manager?;
            (manager, token)
        }
    };
    if let Err(error) = unsafe { manager.ResetDevice(device, token) } {
        log::error!("[WmPresenter] 解码设备挂载失败: {error}");
        return None;
    }
    log::info!("[WmPresenter] 共享解码设备就绪（渲染器设备，代际 {generation}）");
    slot.manager = Some((manager.clone(), token));
    slot.held_generation = generation;
    Some(manager)
}

/// 创建 MediaEngine（frame-server 模式：引擎解码，应用每帧
/// `TransferVideoFrame` 取画面——落点由播放器经视频 quad 提交）。
///
/// 两个属性缺一不可：
/// - `MF_MEDIA_ENGINE_CALLBACK`：事件回调（首帧/缓冲/播完/失败）；
/// - `MF_MEDIA_ENGINE_DXGI_MANAGER` + `MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT`：
///   解码设备与输出像素格式，**合起来决定 frame-server 模式**（不给
///   `PLAYBACK_VISUAL` 就不是 rendering mode）。
pub(super) fn create_engine(
    device: &ID3D11Device,
    generation: u64,
    notify: &IMFMediaEngineNotify,
) -> Result<(IMFMediaEngine, IMFMediaEngineEx)> {
    let factory: IMFMediaEngineClassFactory =
        unsafe { CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_ALL)? };
    let mut store: Option<IMFAttributes> = None;
    unsafe { MFCreateAttributes(&mut store, 3)? };
    let store = store.expect("MFCreateAttributes 返回空属性表");

    let callback: IUnknown = notify.cast()?;
    unsafe {
        store.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &callback)?;
        if let Some(manager) = device_manager(device, generation) {
            store.SetUnknown(&MF_MEDIA_ENGINE_DXGI_MANAGER, &manager)?;
        }
        store.SetUINT32(
            &MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT,
            DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32,
        )?;
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

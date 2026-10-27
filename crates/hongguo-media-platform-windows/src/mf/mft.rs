//! MFT 会话线程与驱动：COM/MTA + MFStartup 生命周期（[`SESSION_INIT`] /
//! [`run_on_mf_thread`]）、MFT 枚举与激活小工具，以及同步（[`SyncMft`]）、
//! 异步（[`AsyncMft`]）两套驱动之上的统一 [`Driver`]。

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
use windows::Win32::System::Variant::{VARIANT, VARIANT_0_0, VARIANT_0_0_0, VT_UI4};
use windows::core::{GUID, Interface};

use hongguo_common::{AppError, AppResult};

use super::decoder::frame_size_of;
use super::probe::MF_VERSION;
use super::sample::type_stride;

// ————————————————————————————————————————————————————————————
// 会话线程：COM/MTA + MFStartup 的生命周期
// ————————————————————————————————————————————————————————————

/// COM(MTA) + MF 的进程级初始化：**一次初始化，永不拆卸**。
///
/// 按会话配对 `CoUninitialize`/`MFShutdown` 会触发 COM 拆卸与 MF 组件后台
/// 线程（实测微软 HEVC 扩展解码器带常驻工作线程）的退出竞态：全量测试 +
/// MF 会话并行的进程里，~1/3 在**全部用例通过之后**的退出阶段于
/// ntdll 固定偏移处访问违例（WER 事件日志实证）。常驻进程（本应用就是）
/// 一次初始化、交给进程退出统一回收是标准做法，没有泄漏语义。
pub(super) static SESSION_INIT: std::sync::Once = std::sync::Once::new();

/// 在专用线程上跑 MF 会话。scoped 线程允许闭包借用非 'static 数据
/// （请求/解复用结果），COM 状态不落在 tokio 工作线程上。
pub(super) fn run_on_mf_thread<T: Send>(f: impl FnOnce() -> AppResult<T> + Send) -> AppResult<T> {
    std::thread::scope(|s| {
        s.spawn(move || {
            SESSION_INIT.call_once(|| {
                // RPC_E_CHANGED_MODE：进程已按别的模式初始化过 COM——
                // 沿用现状即可，MTA 是否由本调用建立不影响会话正确性。
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                if let Err(e) = unsafe { MFStartup(MF_VERSION, 0) } {
                    log::error!("[Platform/mf] MFStartup 失败: {e}");
                }
            });
            f()
        })
        .join()
        .map_err(|_| AppError::Media("Media Foundation 线程异常退出".into()))?
    })
}

/// MF 调用错误的统一包装（对齐 vt 侧的 `os()`）。
pub(super) fn hr<T>(r: windows::core::Result<T>, what: &str) -> AppResult<T> {
    r.map_err(|e| AppError::Media(format!("Media Foundation {what}失败（{e}）")))
}

fn is_code(e: &windows::core::Error, code: windows::core::HRESULT) -> bool {
    e.code() == code
}

/// 经 ICodecAPI 设一个 u32 码控参数（尽力而为，被拒只记日志）。
pub(super) fn set_codecapi_u32(xform: &IMFTransform, api: &GUID, value: u32, label: &str) {
    let Ok(icapi) = xform.cast::<ICodecAPI>() else {
        return;
    };
    let mut v = VARIANT::default();
    v.Anonymous.Anonymous = std::mem::ManuallyDrop::new(VARIANT_0_0 {
        vt: VT_UI4,
        wReserved1: 0,
        wReserved2: 0,
        wReserved3: 0,
        Anonymous: VARIANT_0_0_0 { ulVal: value },
    });
    unsafe {
        if let Err(e) = icapi.SetValue(api, &v) {
            log::debug!("[Platform/mf] 编码器拒收 {label}: {e}");
        }
    }
}

// ————————————————————————————————————————————————————————————
// MFT 枚举与激活
// ————————————————————————————————————————————————————————————

pub(super) fn type_info(major: GUID, subtype: GUID) -> MFT_REGISTER_TYPE_INFO {
    MFT_REGISTER_TYPE_INFO {
        guidMajorType: major,
        guidSubtype: subtype,
    }
}

/// 枚举 MFT（按 `SORTANDFILTER` 的排序）。注意：**没有**
/// `MFT_ENUM_FLAG_SOFTWARE` 这个标志——软件 MFT 用 `SYNCMFT`（不带
/// HARDWARE）枚举，这是对 mfapi.h 核实过的事实，别凭直觉加。
pub(super) fn enum_activates(
    category: GUID,
    flags: MFT_ENUM_FLAG,
    input: Option<&MFT_REGISTER_TYPE_INFO>,
    output: Option<&MFT_REGISTER_TYPE_INFO>,
) -> AppResult<Vec<IMFActivate>> {
    unsafe {
        let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut n = 0u32;
        hr(
            MFTEnumEx(
                category,
                flags,
                input.map(|p| p as *const _),
                output.map(|p| p as *const _),
                &mut list,
                &mut n,
            ),
            "MFTEnumEx",
        )?;
        let mut out = Vec::with_capacity(n as usize);
        if !list.is_null() {
            for i in 0..n as usize {
                if let Some(a) = (*list.add(i)).take() {
                    out.push(a);
                }
            }
            CoTaskMemFree(Some(list as *const _));
        }
        Ok(out)
    }
}

/// 激活一个 MFT。异步契约的 MFT 必须先在属性表上解锁
/// （`MF_TRANSFORM_ASYNC_UNLOCK=1`），否则一切调用都回
/// `MF_E_TRANSFORM_ASYNC_LOCKED`。
fn activate_transform(activate: &IMFActivate) -> AppResult<(IMFTransform, bool)> {
    unsafe {
        let xform: IMFTransform = hr(activate.ActivateObject(), "激活 MFT")?;
        let attrs = hr(xform.GetAttributes(), "读 MFT 属性表")?;
        let is_async = attrs.GetUINT32(&MF_TRANSFORM_ASYNC).unwrap_or(0) != 0;
        if is_async {
            hr(
                attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1),
                "解锁异步 MFT",
            )?;
        }
        Ok((xform, is_async))
    }
}

// ————————————————————————————————————————————————————————————
// 同步 MFT 驱动（in-box 软件 MFT 的契约）
// ————————————————————————————————————————————————————————————

pub(super) enum Pulled {
    NeedMoreInput,
    Sample(IMFSample),
    StreamChange,
}

/// 同步 MFT：输出缓冲要么 MFT 自己给（`PROVIDES_SAMPLES`），要么按
/// `GetOutputStreamInfo` 的 `cbSize` 分配。类型协商后要 `refresh_out_info`。
pub(super) struct SyncMft {
    pub(super) xform: IMFTransform,
    pub(super) provides_samples: bool,
    pub(super) out_size: u32,
}

impl SyncMft {
    pub(super) fn refresh_out_info(&mut self) -> AppResult<()> {
        unsafe {
            let info = hr(self.xform.GetOutputStreamInfo(0), "读输出流信息")?;
            self.provides_samples =
                (info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
            self.out_size = info.cbSize;
        }
        Ok(())
    }

    /// 当前输出类型的几何（stride, 宽, 高）。类型未就绪时 None。
    fn current_geometry(&self) -> Option<(usize, usize, usize)> {
        unsafe {
            let cur = self.xform.GetOutputCurrentType(0).ok()?;
            let (w, h) = frame_size_of(&cur).ok()?;
            Some((type_stride(&cur, w), w, h))
        }
    }

    /// 输出缓冲的分配尺寸：契约下界 cbSize 与「类型几何 + 高度 16 对齐」
    /// 取大者。HEVC 码流的 coded 高几乎恒为 16 的倍数（显示高 1080 实际
    /// 写 1088 行，conformance window 裁边），而 cbSize 由声明几何算出——
    /// 本机 HEVC 扩展对非常规宽度（1924×1080）实测写超声明值，首次取帧
    /// 即 MF_E_BUFFERTOOSMALL。预留对齐头，解析端只读显示区，多出的尾巴
    /// 无害。
    fn out_alloc_size(&self) -> u32 {
        let padded = self
            .current_geometry()
            .map(|(stride, _w, h)| (stride * h.div_ceil(16) * 16 * 3 / 2) as u32)
            .unwrap_or(0);
        self.out_size.max(padded)
    }

    fn alloc_out_sample(&self) -> Option<IMFSample> {
        if self.provides_samples || self.out_size == 0 {
            return None;
        }
        (|| -> AppResult<Option<IMFSample>> {
            unsafe {
                let sample = hr(MFCreateSample(), "建输出样本")?;
                let buf = hr(MFCreateMemoryBuffer(self.out_alloc_size()), "建输出缓冲")?;
                hr(sample.AddBuffer(&buf), "挂输出缓冲")?;
                Ok(Some(sample))
            }
        })()
        .map_err(|e| {
            log::warn!("[Platform/mf] 分配输出缓冲失败: {e}");
            e
        })
        .ok()
        .flatten()
    }

    pub(super) fn pull_once(&mut self) -> AppResult<Pulled> {
        unsafe {
            // BUFFERTOOSMALL 只放大重配重取一次（输出仍挂在 MFT 里，重取
            // 即可）；仍不足说明解码器写出连类型几何都装不下，如实上抛
            let mut retried = false;
            loop {
                let buf = MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: std::mem::ManuallyDrop::new(self.alloc_out_sample()),
                    dwStatus: 0,
                    pEvents: std::mem::ManuallyDrop::new(None),
                };
                let mut arr = [buf];
                let mut status = 0u32;
                let r = self.xform.ProcessOutput(0, &mut arr, &mut status);
                // 所有权先取回再分胜负：`NEED_MORE_INPUT` / `STREAM_CHANGE` 是
                // pull 循环的常态收尾（拉到没输出为止），留在 ManuallyDrop 里
                // 不取 = 每次漏一块 out_size 的原生内存（实测 1080p NV12 一路
                // 每帧 3.1MB、25fps 80MB/s）。pEvents 同理（MFT 塞的事件对象
                // 也归调用方释放）。
                let s = std::mem::ManuallyDrop::take(&mut arr[0].pSample);
                let _events = std::mem::ManuallyDrop::take(&mut arr[0].pEvents);
                match r {
                    Ok(()) => {
                        return Ok(match s {
                            Some(s) => Pulled::Sample(s),
                            None => Pulled::NeedMoreInput,
                        });
                    }
                    Err(e) if is_code(&e, MF_E_TRANSFORM_NEED_MORE_INPUT) => {
                        return Ok(Pulled::NeedMoreInput);
                    }
                    Err(e) if is_code(&e, MF_E_TRANSFORM_STREAM_CHANGE) => {
                        return Ok(Pulled::StreamChange);
                    }
                    Err(e) if is_code(&e, MF_E_BUFFERTOOSMALL) && !retried => {
                        retried = true;
                        let (declared, geom) = (self.out_size, self.current_geometry());
                        self.refresh_out_info()?;
                        log::warn!(
                            "[Platform/mf] ProcessOutput 缓冲不足（声明 cbSize={declared}，类型几何 {geom:?}，刷新后 cbSize={}），按对齐几何放大重配重取",
                            self.out_size
                        );
                    }
                    Err(e) => {
                        return Err(AppError::Media(format!(
                            "Media Foundation ProcessOutput 失败（{e}）"
                        )));
                    }
                }
            }
        }
    }
}

// ————————————————————————————————————————————————————————————
// 异步 MFT 驱动（硬件 MFT 的契约：事件循环）
// ————————————————————————————————————————————————————————————

/// 异步 MFT：只有收到 `METransformNeedInput` 才能喂；`METransformHaveOutput`
/// 后调 `ProcessOutput` 取样本。NeedInput 到来而队列空时记 `need_input`，
/// 下一个入队样本直接补发——错过一个事件就永远喂不进了。
struct AsyncMft {
    xform: IMFTransform,
    event_gen: IMFMediaEventGenerator,
    input_q: VecDeque<IMFSample>,
    need_input: bool,
    drained: bool,
    provides_samples: bool,
    out_size: u32,
}

impl AsyncMft {
    /// 非阻塞收一轮事件。
    fn poll(&mut self, ready: &mut Vec<IMFSample>) -> AppResult<()> {
        unsafe {
            loop {
                let ev = match self.event_gen.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                    Ok(ev) => ev,
                    Err(e) if is_code(&e, MF_E_NO_EVENTS_AVAILABLE) => break,
                    Err(e) => {
                        return Err(AppError::Media(format!(
                            "Media Foundation 事件循环失败（{e}）"
                        )));
                    }
                };
                let ty = MF_EVENT_TYPE(ev.GetType().map_err(|e| {
                    AppError::Media(format!("Media Foundation 读事件类型失败（{e}）"))
                })? as i32);
                match ty {
                    METransformNeedInput => match self.input_q.pop_front() {
                        Some(s) => hr(self.xform.ProcessInput(0, &s, 0), "ProcessInput")?,
                        None => {
                            self.need_input = true;
                            break;
                        }
                    },
                    METransformHaveOutput => ready.push(self.pull_one()?),
                    METransformMarker => {}
                    METransformDrainComplete => {
                        self.drained = true;
                        break;
                    }
                    _ => {}
                }
            }
            if self.need_input
                && let Some(s) = self.input_q.pop_front()
            {
                hr(self.xform.ProcessInput(0, &s, 0), "ProcessInput")?;
                self.need_input = false;
            }
        }
        Ok(())
    }

    /// 输出缓冲的分配尺寸：契约下界 cbSize 与「类型几何 + 高度 16 对齐」
    /// 取大者（同 [`SyncMft::out_alloc_size`]——异步 MFT 同样可能写超
    /// 声明值）。
    fn out_alloc_size(&self) -> u32 {
        let padded = unsafe {
            self.xform.GetOutputCurrentType(0).ok().and_then(|cur| {
                frame_size_of(&cur)
                    .ok()
                    .map(|(w, h)| (type_stride(&cur, w) * h.div_ceil(16) * 16 * 3 / 2) as u32)
            })
        }
        .unwrap_or(0);
        self.out_size.max(padded)
    }

    fn pull_one(&mut self) -> AppResult<IMFSample> {
        unsafe {
            // 与 SyncMft::pull_once 同款：缓冲不足按刷新后的契约放大重配，
            // 只重取一次
            let mut retried = false;
            loop {
                let sample = if self.provides_samples {
                    None
                } else {
                    let size = self.out_alloc_size();
                    let s = hr(MFCreateSample(), "建输出样本")?;
                    let buf = hr(MFCreateMemoryBuffer(size), "建输出缓冲")?;
                    hr(s.AddBuffer(&buf), "挂输出缓冲")?;
                    Some(s)
                };
                let mut arr = [MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: std::mem::ManuallyDrop::new(sample),
                    dwStatus: 0,
                    pEvents: std::mem::ManuallyDrop::new(None),
                }];
                let mut status = 0u32;
                let r = self.xform.ProcessOutput(0, &mut arr, &mut status);
                // 所有权先取回再核错误（同 SyncMft::pull_once 的泄漏教训）：
                // `?` 提前返回会把 ManuallyDrop 里的样本/事件漏在原地。
                let s = std::mem::ManuallyDrop::take(&mut arr[0].pSample);
                let _events = std::mem::ManuallyDrop::take(&mut arr[0].pEvents);
                match r {
                    Ok(()) => {
                        return s.ok_or_else(|| {
                            AppError::Media("Media Foundation 输出样本缺失".into())
                        });
                    }
                    Err(e) if is_code(&e, MF_E_BUFFERTOOSMALL) && !retried => {
                        retried = true;
                        let declared = self.out_size;
                        let info = hr(self.xform.GetOutputStreamInfo(0), "读输出流信息")?;
                        self.provides_samples =
                            (info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
                        self.out_size = info.cbSize;
                        log::warn!(
                            "[Platform/mf] ProcessOutput 缓冲不足（声明 cbSize={declared}，刷新后 {}），放大重配重取",
                            self.out_size
                        );
                    }
                    Err(e) => {
                        return Err(AppError::Media(format!(
                            "Media Foundation ProcessOutput 失败（{e}）"
                        )));
                    }
                }
            }
        }
    }

    /// 排空异步 MFT。顺序契约：
    /// ① **把队列里没喂完的样本全部喂完**才能发 `END_OF_STREAM`——NeedInput
    ///    事件的到达可能滞后（实测首个会话之后事件明显变慢），提早宣告流
    ///    结束会让 MFT 只编码已收到的帧、剩下的整段丢掉；
    /// ② `END_OF_STREAM` 之后**还要**发 `COMMAND_DRAIN`，否则有的 MFT
    ///    （实测 NVIDIA 编码 MFT）永远不发 `METransformDrainComplete`。
    /// 全程带超时兜底（MFT 卡死时不拖死整个转码）。
    fn drain(&mut self, ready: &mut Vec<IMFSample>) -> AppResult<()> {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !self.input_q.is_empty() && !self.drained {
            if Instant::now() > deadline {
                return Err(AppError::Media("Media Foundation 异步 MFT 喂入超时".into()));
            }
            self.poll(ready)?;
            if !self.input_q.is_empty() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        unsafe {
            hr(
                self.xform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0),
                "通知流结束",
            )?;
            hr(
                self.xform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0),
                "命令排空",
            )?;
        }
        while !self.drained {
            if Instant::now() > deadline {
                return Err(AppError::Media("Media Foundation 异步 MFT 收尾超时".into()));
            }
            self.poll(ready)?;
            if !self.drained {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        self.poll(ready)
    }
}

// ————————————————————————————————————————————————————————————
// 统一驱动：解码器/编码器不关心同步还是异步
// ————————————————————————————————————————————————————————————

/// MFT 的统一驱动：输入侧 `push`，输出侧攒在 `ready` 里由 `take` 收割。
/// 同步 MFT 解码中可能报输出类型变化（分辨率变化流），置 sticky 标志
/// `stream_changed`，由 Decoder 重谈输出类型后继续。
pub(super) struct Driver {
    inner: DriverInner,
    ready: Vec<IMFSample>,
    pub(super) stream_changed: bool,
}

enum DriverInner {
    Sync(SyncMft),
    Async(AsyncMft),
}

impl Driver {
    pub(super) fn new(activate: &IMFActivate) -> AppResult<(Self, IMFTransform)> {
        let (xform, is_async) = activate_transform(activate)?;
        let inner = if is_async {
            let event_gen: IMFMediaEventGenerator = hr(xform.cast(), "取事件生成器")?;
            DriverInner::Async(AsyncMft {
                xform: xform.clone(),
                event_gen,
                input_q: VecDeque::new(),
                need_input: false,
                drained: false,
                provides_samples: false,
                out_size: 0,
            })
        } else {
            DriverInner::Sync(SyncMft {
                xform: xform.clone(),
                provides_samples: false,
                out_size: 0,
            })
        };
        Ok((
            Driver {
                inner,
                ready: Vec::new(),
                stream_changed: false,
            },
            xform,
        ))
    }

    /// 类型协商完成后调用：刷新输出流信息 + 开流。
    pub(super) fn begin_streaming(&mut self) -> AppResult<()> {
        unsafe {
            match &mut self.inner {
                DriverInner::Sync(s) => {
                    s.refresh_out_info()?;
                    hr(
                        s.xform
                            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0),
                        "开始流式处理",
                    )?;
                    // 同步 MFT 也补发 START_OF_STREAM：实测 h264_mf 不发这条
                    // 会把输入帧当不存在，只吐 SPS/PPS/AUD 不吐 slice。
                    hr(
                        s.xform
                            .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0),
                        "通知流开始",
                    )?;
                }
                DriverInner::Async(a) => {
                    let info = hr(a.xform.GetOutputStreamInfo(0), "读输出流信息")?;
                    a.provides_samples =
                        (info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
                    a.out_size = info.cbSize;
                    log::info!(
                        "[Platform/mf] 异步 MFT 输出契约：provides_samples={}，单帧缓冲 {}B",
                        a.provides_samples,
                        a.out_size
                    );
                    hr(
                        a.xform
                            .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0),
                        "开始流式处理",
                    )?;
                    hr(
                        a.xform
                            .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0),
                        "通知流开始",
                    )?;
                }
            }
        }
        Ok(())
    }

    /// 喂一个输入样本，顺带收割已就绪的输出。
    pub(super) fn push(&mut self, sample: IMFSample) -> AppResult<()> {
        match &mut self.inner {
            DriverInner::Sync(s) => unsafe {
                hr(s.xform.ProcessInput(0, &sample, 0), "ProcessInput")?;
                loop {
                    match s.pull_once()? {
                        Pulled::Sample(sm) => self.ready.push(sm),
                        Pulled::NeedMoreInput => break,
                        Pulled::StreamChange => {
                            self.stream_changed = true;
                            break;
                        }
                    }
                }
                Ok(())
            },
            DriverInner::Async(a) => {
                a.input_q.push_back(sample);
                a.poll(&mut self.ready)
            }
        }
    }

    /// 只收割输出，不喂输入（类型重谈后 / 收尾后用）。
    pub(super) fn harvest(&mut self) -> AppResult<()> {
        match &mut self.inner {
            DriverInner::Sync(s) => {
                loop {
                    match s.pull_once()? {
                        Pulled::Sample(sm) => self.ready.push(sm),
                        Pulled::NeedMoreInput => break,
                        Pulled::StreamChange => {
                            self.stream_changed = true;
                            break;
                        }
                    }
                }
                Ok(())
            }
            DriverInner::Async(a) => a.poll(&mut self.ready),
        }
    }

    /// 结束输入并冲出全部剩余输出。
    pub(super) fn finish(&mut self) -> AppResult<()> {
        match &mut self.inner {
            DriverInner::Sync(s) => unsafe {
                hr(
                    s.xform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0),
                    "通知流结束",
                )?;
                hr(
                    s.xform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0),
                    "命令排空",
                )?;
                loop {
                    match s.pull_once()? {
                        Pulled::Sample(sm) => self.ready.push(sm),
                        Pulled::NeedMoreInput => break,
                        Pulled::StreamChange => {
                            self.stream_changed = true;
                            break;
                        }
                    }
                }
                Ok(())
            },
            DriverInner::Async(a) => a.drain(&mut self.ready),
        }
    }

    pub(super) fn take(&mut self) -> Vec<IMFSample> {
        std::mem::take(&mut self.ready)
    }
}

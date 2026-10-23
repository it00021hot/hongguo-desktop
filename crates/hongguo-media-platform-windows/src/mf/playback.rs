//! 播放专用解码 MFT：HEVC / H.264 → NV12 系统内存帧、AAC → 交错 f32。
//!
//! 转码链（[`super`]）的解码走 SourceReader 文件流；播放没有文件——样本经
//! serve 逐段进内存，这里**直接驱动解码 MFT**（复用 [`super`] 的 Driver 统一
//! 驱动、MFT 枚举/激活与样本工具）。每个解码器一条常驻线程：COM/MTA 状态
//! 不落在 tokio 工作线程上，请求-应答式阻塞调用与 macOS 侧 VT 的异步回调
//! 等价（调用方都在引擎的解码循环里串行推进）。
//!
//! 两个本机实证的契约（详见各函数头注释）：HEVC / H.264 解码器只吃
//! **Annex-B 起始码**样本（长度前缀被无声吞掉）；AAC 的 `MF_MT_USER_DATA` 是 **12 字节
//! HEAACWAVEINFO 尾巴 + ASC**（裸 ASC 被拒），输入/输出类型都要从解码器
//! 枚举的**可用类型**里选完整形态。

use std::sync::mpsc;

use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
use windows::core::{GUID, PWSTR};

use hongguo_common::{AppError, AppResult};
use hongguo_media_platform_api::video::VideoCodec;

use super::sample::{finish_sample, locked_buffer};
use super::{
    Driver, MF_VERSION, SESSION_INIT, enum_activates, frame_size_of, hr, sample_to_bytes,
    secs_to_hns, type_info, type_stride, video_type,
};

/// 解码线程的一次会话初始化（进程级一次，永不拆卸——理由见
/// [`super::SESSION_INIT`] 的注释）。
fn session_init_once() {
    SESSION_INIT.call_once(|| {
        // RPC_E_CHANGED_MODE：沿用进程既有 COM 模式即可
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if let Err(e) = unsafe { MFStartup(MF_VERSION, 0) } {
            log::error!("[Platform/mf] MFStartup 失败: {e}");
        }
    });
}

/// 喂入字节数据 → IMFSample（压缩样本共用的构造：锁缓冲、拷贝、定时戳）。
/// 建样本骨架（MFCreateSample→建缓冲→Lock→拷贝→Unlock→SetCurrentLength→
/// AddBuffer→SetSampleTime/Duration）与 [`super::sample`] 的 nv12_sample
/// 共用 locked_buffer/finish_sample 实现，只是源是裸字节不是 NV12 平面。
unsafe fn bytes_sample(data: &[u8], pts_100ns: i64, dur_100ns: i64) -> AppResult<IMFSample> {
    unsafe {
        let (sample, buf, ptr) = locked_buffer(data.len())?;
        std::slice::from_raw_parts_mut(ptr, data.len()).copy_from_slice(data);
        finish_sample(&sample, &buf, data.len(), pts_100ns, dur_100ns)?;
        Ok(sample)
    }
}

/// 长度前缀样本流 → Annex-B：每个 NALU 的 `length_size` 前缀换成
/// `00 00 00 01` 起始码。**HEVC 扩展解码器实测只吃 Annex-B 形态**——
/// `MFVideoFormat_HEVC` 的文档口径是长度前缀，但本机（HEVCVideoExtension
/// 2.5.x）直驱 `IMFTransform` 时长度前缀样本被无声吞掉（喂 244 个零输出），
/// 起始码形态立即出帧（与 H.264 时代 mf 解码器只认 Annex-B 同款脾气）。
/// 解析不出合法长度（首 4 字节超出总长/长度为 0）时原样返回，交给解码器
/// 报错而不是在这里猜。边界语义与参数集侧的 [`super::annexb::split_avcc`]
/// （moov/定长 4 字节截断）有意不同，对照见其注释。
fn annexb_from_length_prefixed(data: &[u8], length_size: usize) -> Vec<u8> {
    let n = length_size.clamp(1, 4);
    let mut out = Vec::with_capacity(data.len());
    let mut pos = 0usize;
    while pos + n <= data.len() {
        let mut len = 0usize;
        for b in &data[pos..pos + n] {
            len = (len << 8) | *b as usize;
        }
        if len == 0 || pos + n + len > data.len() {
            return data.to_vec();
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&data[pos + n..pos + n + len]);
        pos += n + len;
    }
    if pos != data.len() {
        return data.to_vec();
    }
    out
}

/// 一个解码出的 NV12 帧（系统内存，带字节行距）。
pub(crate) struct Nv12Out {
    pub data: Vec<u8>,
    pub stride: usize,
    pub width: usize,
    pub height: usize,
    pub pts_secs: f64,
}

/// 输出可见区（`MF_MT_GEOMETRIC_APERTURE`）。
///
/// 解码器对某些码流（本机实测：H.264 非 16 整倍数尺寸，x264 把 360 宽码到
/// 368）按**编码尺寸**出帧，可见画面只占其中一块——不裁的话右/下缘的
/// padding 会被当画面渲染（本机实测是一条绿边）。`None` = 整帧可见。
#[derive(Clone, Copy, Debug)]
struct Aperture {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

/// 从输出类型读 `MF_MT_GEOMETRIC_APERTURE`。载荷是 `MFVideoArea`：
/// 两个 `MFOffset`（u16 小数 + i16 整数，共 8 字节）+ `SIZE`（i32 宽高，
/// 8 字节）。缺省、越界、等于整帧时返回 `None`；偏移取整数部分并按 NV12
/// 半采样取偶。
unsafe fn aperture_of(t: &IMFMediaType, coded_w: usize, coded_h: usize) -> Option<Aperture> {
    let mut blob = [0u8; 16];
    let mut size = 0u32;
    unsafe {
        t.GetBlob(
            &MF_MT_GEOMETRIC_APERTURE,
            &mut blob,
            Some(&mut size as *mut u32),
        )
        .ok()?;
    }
    if size < 16 {
        return None;
    }
    let x = i16::from_le_bytes([blob[2], blob[3]]).max(0) as usize & !1;
    let y = i16::from_le_bytes([blob[6], blob[7]]).max(0) as usize & !1;
    let w = i32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]);
    let h = i32::from_le_bytes([blob[12], blob[13], blob[14], blob[15]]);
    if w <= 0 || h <= 0 {
        return None;
    }
    let w = w as usize & !1;
    let h = h as usize & !1;
    if w == 0 || h == 0 || x + w > coded_w || y + h > coded_h {
        return None;
    }
    if x == 0 && y == 0 && w == coded_w && h == coded_h {
        return None;
    }
    Some(Aperture { x, y, w, h })
}

/// 把可见区从 coded 帧里裁出来（NV12 行拷贝；可见区贴着左上角，
/// 右侧/底部是编码 padding）。产物行距 = 可见宽。
fn crop_to_aperture(data: &[u8], stride: usize, coded_h: usize, a: Aperture) -> Vec<u8> {
    let mut out = vec![0u8; a.w * a.h * 3 / 2];
    let (luma, chroma) = out.split_at_mut(a.w * a.h);
    for row in 0..a.h {
        let s = (a.y + row) * stride + a.x;
        luma[row * a.w..(row + 1) * a.w].copy_from_slice(&data[s..s + a.w]);
    }
    // UV 行与列都是半采样：字节偏移 = 像素偏移（U+V 两字节一像素对）
    let uv_src = stride * coded_h + (a.y / 2) * stride + a.x;
    for row in 0..a.h / 2 {
        let s = uv_src + row * stride;
        chroma[row * a.w..(row + 1) * a.w].copy_from_slice(&data[s..s + a.w]);
    }
    out
}

/// 解码线程的请求/应答（视频与音频共用形态，各自只用到相关变体）。
enum Req {
    /// 喂一个样本，收本次带出的输出。
    Decode {
        sample: Vec<u8>,
        pts: f64,
        dur: f64,
    },
    /// 冲尾（END_OF_STREAM + DRAIN），视频收尾用。
    Finish,
    /// 冲流不收尾（音频 seek 用：丢内部缓冲，继续接新位置的包）。
    Flush,
    Shutdown,
}

enum Rsp {
    Frames(Vec<Nv12Out>),
    Samples(Vec<f32>),
    Done,
    Err(AppError),
}

/// 会话失败后的兜底应答：给每个未决请求回同一个错误（转成 String 再造，
/// AppError 不 Clone），调用方不至于在 recv 上挂死。
fn replay_error(rx: &mpsc::Receiver<Req>, tx: &mpsc::Sender<Rsp>, msg: String) {
    while let Ok(req) = rx.recv() {
        match req {
            Req::Shutdown => break,
            _ => {
                let _ = tx.send(Rsp::Err(AppError::Media(msg.clone())));
            }
        }
    }
}

// ————————————————————————————————————————————————————————————
// 视频（HEVC / H.264）
// ————————————————————————————————————————————————————————————

/// 播放用视频解码器（HEVC / H.264）：专用线程持有 MFT，请求-应答式驱动。
pub(crate) struct VideoMftDecoder {
    tx: mpsc::Sender<Req>,
    rx: mpsc::Receiver<Rsp>,
}

impl VideoMftDecoder {
    /// `sequence_header` 是序列头原始载荷（HEVC 的 hvcC / H.264 的 avcC，
    /// `MF_MT_MPEG_SEQUENCE_HEADER` 的指定形态）；`parameter_sets`/`length_size`
    /// 用来给首样本前插带内参数集——个别解码器拒收序列头 blob 时，带内参数集
    /// 是第二道保险。
    pub(crate) fn new(
        codec: VideoCodec,
        sequence_header: Option<&[u8]>,
        parameter_sets: &[Vec<u8>],
        length_size: usize,
        width: usize,
        height: usize,
    ) -> AppResult<Self> {
        let subtype = match codec {
            VideoCodec::Hevc => MFVideoFormat_HEVC,
            VideoCodec::H264 => MFVideoFormat_H264,
        };
        let (tx_req, rx_req) = mpsc::channel::<Req>();
        let (tx_rsp, rx_rsp) = mpsc::channel::<Rsp>();
        let spec = VideoSessionSpec {
            subtype,
            label: if subtype == MFVideoFormat_HEVC {
                "HEVC"
            } else {
                "H.264"
            },
            sequence_header: sequence_header.map(<[u8]>::to_vec),
            parameter_sets: parameter_sets.to_vec(),
            length_size,
            width,
            height,
        };
        let spawned = std::thread::Builder::new()
            .name(match codec {
                VideoCodec::Hevc => "hg-mf-hevc".into(),
                VideoCodec::H264 => "hg-mf-h264".into(),
            })
            .spawn(move || {
                session_init_once();
                if let Err(e) = video_session(spec, &rx_req, &tx_rsp) {
                    replay_error(&rx_req, &tx_rsp, e.to_string());
                }
            });
        if let Err(e) = spawned {
            return Err(AppError::Media(format!("视频解码线程起不来: {e}")));
        }
        Ok(Self {
            tx: tx_req,
            rx: rx_rsp,
        })
    }

    fn call(&mut self, req: Req) -> AppResult<Vec<Nv12Out>> {
        self.tx
            .send(req)
            .map_err(|_| AppError::Media("视频解码线程已退出".into()))?;
        match self.rx.recv() {
            Ok(Rsp::Frames(frames)) => Ok(frames),
            Ok(Rsp::Err(e)) => Err(e),
            Ok(_) => Ok(Vec::new()),
            Err(_) => Err(AppError::Media("视频解码线程应答缺失".into())),
        }
    }

    pub(crate) fn decode(
        &mut self,
        sample: &[u8],
        pts_secs: f64,
        dur_secs: f64,
    ) -> AppResult<Vec<Nv12Out>> {
        self.call(Req::Decode {
            sample: sample.to_vec(),
            pts: pts_secs,
            dur: dur_secs,
        })
    }

    pub(crate) fn finish(&mut self) -> AppResult<Vec<Nv12Out>> {
        self.call(Req::Finish)
    }
}

impl Drop for VideoMftDecoder {
    fn drop(&mut self) {
        let _ = self.tx.send(Req::Shutdown);
    }
}

/// 设 NV12 输出类型。优先用解码器**自己枚举的可用类型**（属性齐全——
/// 手工构造的裸 major+subtype 在个别硬解 MFT 上被 `MF_E_ATTRIBUTENOTFOUND`
/// 拒收，它要的类型属性我们没有）；枚举不出 NV12 再回落自建类型（带帧
/// 尺寸，软件 MFT 接受这种形态）。
unsafe fn negotiate_nv12_output(
    xform: &IMFTransform,
    width: usize,
    height: usize,
) -> AppResult<()> {
    unsafe {
        let mut idx = 0u32;
        while let Ok(t) = xform.GetOutputAvailableType(0, idx) {
            if t.GetGUID(&MF_MT_SUBTYPE)
                .is_ok_and(|g| g == MFVideoFormat_NV12)
            {
                return hr(xform.SetOutputType(0, &t, 0), "设 NV12 解码输出");
            }
            idx += 1;
        }
        let out_t = video_type(MFVideoFormat_NV12)?;
        hr(
            out_t.SetUINT64(&MF_MT_FRAME_SIZE, (width as u64) << 32 | height as u64),
            "设解码帧尺寸",
        )?;
        hr(xform.SetOutputType(0, &out_t, 0), "设 NV12 解码输出")
    }
}

/// MFT 的友好名（诊断日志用：候选迭代时能看出走的是硬解还是扩展解码器）。
fn activate_name(activate: &IMFActivate) -> String {
    unsafe {
        let mut name = PWSTR::null();
        let mut len = 0u32;
        let got = activate
            .GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut name, &mut len)
            .is_ok()
            && !name.is_null();
        let out = if got {
            name.to_string().unwrap_or_else(|_| "未命名解码器".into())
        } else {
            "未命名解码器".into()
        };
        if got {
            CoTaskMemFree(Some(name.as_ptr() as *const _));
        }
        out
    }
}

/// 与一个视频解码 MFT 建立完整会话：输入类型（序列头 blob 被拒时去 blob
/// 重试，带内参数集兜底——见首样本前插）→ NV12 输出 → 开流，返回
/// `(驱动, 变换, 宽, 高, 行距, 可见区)`。任何一步被拒即失败，由调用侧换
/// 下一候选。
unsafe fn open_video_decoder(
    activate: &IMFActivate,
    subtype: GUID,
    sequence_header: &Option<Vec<u8>>,
    width: usize,
    height: usize,
) -> AppResult<(Driver, IMFTransform, usize, usize, usize, Option<Aperture>)> {
    unsafe {
        let (mut driver, xform) = Driver::new(activate)?;
        let in_t = video_type(subtype)?;
        let frame_size = (width as u64) << 32 | height as u64;
        hr(
            in_t.SetUINT64(&MF_MT_FRAME_SIZE, frame_size),
            "设解码帧尺寸",
        )?;
        if let Some(sequence_header) = sequence_header {
            hr(
                in_t.SetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, sequence_header),
                "设视频序列头",
            )?;
        }
        if let Err(e) = xform.SetInputType(0, &in_t, 0) {
            if sequence_header.is_some() {
                log::warn!("[Platform/mf] 序列头被拒（{e}），改走带内参数集");
                let bare = video_type(subtype)?;
                hr(
                    bare.SetUINT64(&MF_MT_FRAME_SIZE, frame_size),
                    "设解码帧尺寸",
                )?;
                hr(xform.SetInputType(0, &bare, 0), "设解码输入类型")?;
            } else {
                return Err(AppError::Media(format!(
                    "Media Foundation 设解码输入类型失败（{e}）"
                )));
            }
        }
        negotiate_nv12_output(&xform, width, height)?;
        let cur = hr(xform.GetOutputCurrentType(0), "读解码输出类型")?;
        let (w, h) = frame_size_of(&cur)?;
        let stride = type_stride(&cur, w);
        let aperture = aperture_of(&cur, w, h);
        driver.begin_streaming()?;
        Ok((driver, xform, w, h, stride, aperture))
    }
}

/// 建视频会话的静态参数（收拢进一个结构，免 `video_session` 参数过界）。
struct VideoSessionSpec {
    subtype: GUID,
    label: &'static str,
    sequence_header: Option<Vec<u8>>,
    parameter_sets: Vec<Vec<u8>>,
    length_size: usize,
    width: usize,
    height: usize,
}

/// 视频会话主体：建 MFT、协商类型、循环应答。返回 Err = 建会话失败
/// （由调用侧回放给所有未决请求）。
fn video_session(
    spec: VideoSessionSpec,
    rx: &mpsc::Receiver<Req>,
    tx: &mpsc::Sender<Rsp>,
) -> AppResult<()> {
    let VideoSessionSpec {
        subtype,
        label,
        sequence_header,
        parameter_sets,
        length_size,
        width,
        height,
    } = spec;
    unsafe {
        let decoders = enum_activates(
            MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG_SYNCMFT
                | MFT_ENUM_FLAG_ASYNCMFT
                | MFT_ENUM_FLAG_HARDWARE
                | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&type_info(MFMediaType_Video, subtype)),
            Some(&type_info(MFMediaType_Video, MFVideoFormat_NV12)),
        )?;
        if decoders.is_empty() {
            return Err(AppError::Media(if label == "HEVC" {
                "没有 HEVC 解码 MFT（装系统的「HEVC 视频扩展」后重试）".into()
            } else {
                "没有 H.264 解码 MFT（系统解码器缺失，属异常机器）".into()
            }));
        }
        // 候选逐个协商，第一个全程谈下来的用（SORTANDFILTER 把硬解排前，
        // 但个别硬解 MFT 对类型形态挑剔/驱动态不稳，谈崩就落到扩展软解）
        let mut last_err = String::new();
        let mut opened: Option<(Driver, IMFTransform, usize, usize, usize, Option<Aperture>)> =
            None;
        for activate in &decoders {
            match open_video_decoder(activate, subtype, &sequence_header, width, height) {
                Ok((driver, xform, w, h, stride, aperture)) => {
                    log::info!(
                        "[Platform/mf] 选中解码器「{}」（{}，候选 {} 个）",
                        activate_name(activate),
                        label,
                        decoders.len()
                    );
                    opened = Some((driver, xform, w, h, stride, aperture));
                    break;
                }
                Err(e) => {
                    log::info!(
                        "[Platform/mf] 解码器「{}」协商失败（{e}），试下一个候选",
                        activate_name(activate)
                    );
                    last_err = e.to_string();
                }
            }
        }
        let Some((mut driver, xform, mut w, mut h, mut stride, mut aperture)) = opened else {
            return Err(AppError::Media(format!(
                "{label} 解码 MFT 全部协商失败（{last_err}）"
            )));
        };
        log::info!(
            "[Platform/mf] 解码会话就绪（{}，{w}x{h}，stride {stride}）",
            label
        );
        if let Some(a) = aperture {
            log::info!(
                "[Platform/mf] 输出按可见区裁剪：{}x{}@({}, {})（编码尺寸 {w}x{h}）",
                a.w,
                a.h,
                a.x,
                a.y
            );
        }

        // 首样本前插带内参数集（MP4 样本自身不带 VPS/SPS/PPS）；前缀长度
        // 按序列头声明的 length_size 构造（1–4 字节）
        let mut first_sample = true;
        let mut inband_params = Vec::new();
        for nalu in &parameter_sets {
            let len = (nalu.len() as u32).to_be_bytes();
            let skip = 4 - length_size.clamp(1, 4);
            inband_params.extend_from_slice(&len[skip..]);
            inband_params.extend_from_slice(nalu);
        }
        // 吞吐哨兵：喂进/产出计数（首帧迟迟不出现时一眼看出是「只进不出」）
        let mut fed = 0usize;
        let mut out_total = 0usize;

        while let Ok(req) = rx.recv() {
            match req {
                Req::Shutdown => return Ok(()),
                Req::Flush => {
                    // 视频侧不走冲流（引擎 seek 重建会话），收到当空应答
                    let _ = xform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
                    let _ = tx.send(Rsp::Frames(Vec::new()));
                }
                Req::Finish => {
                    if let Err(e) = driver.finish() {
                        let _ = tx.send(Rsp::Err(e));
                        continue;
                    }
                    let mut frames = Vec::new();
                    for s in driver.take() {
                        if let Ok(f) = nv12_of(&s, stride, w, h, aperture) {
                            frames.push(f);
                        }
                    }
                    let _ = tx.send(Rsp::Frames(frames));
                }
                Req::Decode { sample, pts, dur } => {
                    let mut data = Vec::with_capacity(inband_params.len() + sample.len());
                    if first_sample {
                        data.extend_from_slice(&inband_params);
                        first_sample = false;
                    }
                    data.extend_from_slice(&sample);
                    fed += 1;
                    if fed.is_multiple_of(100) {
                        log::debug!("[Platform/mf] {label} 已喂 {fed} 样本，累计出帧 {out_total}");
                    }
                    // 长度前缀 → Annex-B（解码器只认起始码形态，见转换函数头注释）
                    let data = annexb_from_length_prefixed(&data, length_size);
                    let input = match bytes_sample(&data, secs_to_hns(pts), secs_to_hns(dur)) {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = tx.send(Rsp::Err(e));
                            continue;
                        }
                    };
                    if let Err(e) = driver.push(input) {
                        // 失败样本取证：长度 + 首部字节 + 时刻（对照样本表定位
                        // 是数据问题还是解码器状态问题）
                        log::warn!(
                            "[Platform/mf] {label} 样本 {fed}（{}B，首 {:02x?}，pts {pts:.3}s）解码被拒",
                            data.len(),
                            &data[..data.len().min(12)],
                        );
                        let _ = tx.send(Rsp::Err(e));
                        continue;
                    }
                    if driver.stream_changed {
                        // 输出类型重谈（分辨率/行距变化）：与建会话同一条协商
                        // 路径（可用类型里挑 NV12），成功后刷新几何
                        if let Err(e) = negotiate_nv12_output(&xform, w, h) {
                            let _ = tx.send(Rsp::Err(AppError::Media(format!(
                                "Media Foundation 重设输出类型失败（{e}）"
                            ))));
                            continue;
                        }
                        if let Ok(cur) = xform.GetOutputCurrentType(0)
                            && let Ok((nw, nh)) = frame_size_of(&cur)
                        {
                            w = nw;
                            h = nh;
                            stride = type_stride(&cur, w);
                            aperture = aperture_of(&cur, w, h);
                            if aperture.is_none() {
                                log::debug!(
                                    "[Platform/mf] 输出类型更新（{w}x{h}，stride {stride}，整帧可见）"
                                );
                            }
                        }
                        driver.stream_changed = false;
                        let _ = driver.harvest();
                    }
                    let mut frames = Vec::new();
                    for s in driver.take() {
                        match nv12_of(&s, stride, w, h, aperture) {
                            Ok(f) => frames.push(f),
                            Err(e) => log::warn!("[Platform/mf] 丢一帧无法解析的输出: {e}"),
                        }
                    }
                    out_total += frames.len();
                    let _ = tx.send(Rsp::Frames(frames));
                }
            }
        }
        Ok(())
    }
}

/// 输出样本 → NV12 帧。MF 按 NV12 契约输出（Y 平面后跟 U,V 交错），这里直通
/// 即可——**不做色度交换**（颜色问题的归属在渲染上传侧，见 hongguo-gpui 的
/// `image_upload::render_image`）。PTS 读样本自带的 100ns 时间戳（喂进去的
/// 媒体时间）；缺失（-1）按 0 处理——引擎侧重排缓冲只认我们自己喂的 pts 序。
/// 解码器声明了可见区（见 [`Aperture`]）时按可见区裁剪，产物行距 = 可见宽。
unsafe fn nv12_of(
    s: &IMFSample,
    stride: usize,
    width: usize,
    height: usize,
    aperture: Option<Aperture>,
) -> AppResult<Nv12Out> {
    let (data, t, _) = sample_to_bytes(s)?;
    let need = stride * height * 3 / 2;
    if data.len() < need {
        return Err(AppError::Media(format!(
            "NV12 输出帧字节不足（{} < 需要 {need}）",
            data.len()
        )));
    }
    let (data, stride, width, height) = match aperture {
        Some(a) => (crop_to_aperture(&data, stride, height, a), a.w, a.w, a.h),
        None => (data, stride, width, height),
    };
    Ok(Nv12Out {
        data,
        stride,
        width,
        height,
        pts_secs: if t >= 0 { t as f64 / 10_000_000.0 } else { 0.0 },
    })
}

// ————————————————————————————————————————————————————————————

/// 播放用 AAC 解码器：in-box 软件 MFT（同步），输出交错 f32。
pub(crate) struct AacMftDecoder {
    tx: mpsc::Sender<Req>,
    rx: mpsc::Receiver<Rsp>,
}

impl AacMftDecoder {
    pub(crate) fn new(asc: &[u8], channels: u16, sample_rate: u32) -> AppResult<Self> {
        let (tx_req, rx_req) = mpsc::channel::<Req>();
        let (tx_rsp, rx_rsp) = mpsc::channel::<Rsp>();
        let asc = asc.to_vec();
        let spawned = std::thread::Builder::new()
            .name("hg-mf-aac".into())
            .spawn(move || {
                session_init_once();
                if let Err(e) = audio_session(&asc, channels, sample_rate, &rx_req, &tx_rsp) {
                    replay_error(&rx_req, &tx_rsp, e.to_string());
                }
            });
        if let Err(e) = spawned {
            return Err(AppError::Media(format!("AAC 解码线程起不来: {e}")));
        }
        Ok(Self {
            tx: tx_req,
            rx: rx_rsp,
        })
    }

    fn call(&mut self, req: Req) -> AppResult<Vec<f32>> {
        self.tx
            .send(req)
            .map_err(|_| AppError::Media("AAC 解码线程已退出".into()))?;
        match self.rx.recv() {
            Ok(Rsp::Samples(s)) => Ok(s),
            Ok(Rsp::Err(e)) => Err(e),
            Ok(_) => Ok(Vec::new()),
            Err(_) => Err(AppError::Media("AAC 解码线程应答缺失".into())),
        }
    }

    pub(crate) fn decode(&mut self, packet: &[u8]) -> AppResult<Vec<f32>> {
        self.call(Req::Decode {
            sample: packet.to_vec(),
            pts: -1.0,
            dur: -1.0,
        })
    }

    /// seek 冲流：丢掉 MFT 内部缓冲（时间轴换轨），继续接新位置的包。
    pub(crate) fn reset(&mut self) {
        let _ = self.call(Req::Flush);
    }
}

impl Drop for AacMftDecoder {
    fn drop(&mut self) {
        let _ = self.tx.send(Req::Shutdown);
    }
}

fn audio_session(
    asc: &[u8],
    channels: u16,
    sample_rate: u32,
    rx: &mpsc::Receiver<Req>,
    tx: &mpsc::Sender<Rsp>,
) -> AppResult<()> {
    unsafe {
        let decoders = enum_activates(
            MFT_CATEGORY_AUDIO_DECODER,
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&type_info(MFMediaType_Audio, MFAudioFormat_AAC)),
            Some(&type_info(MFMediaType_Audio, MFAudioFormat_Float)),
        )?;
        let Some(activate) = decoders.first() else {
            return Err(AppError::Media("没有 AAC 解码 MFT".into()));
        };
        let (mut driver, xform) = Driver::new(activate)?;

        // 输入：裸 AAC 包（PAYLOAD_TYPE=0，非 ADTS），ASC 走 MF_MT_USER_DATA
        let in_t = hr(MFCreateMediaType(), "建音频输入类型")?;
        hr(
            in_t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio),
            "设音频 major type",
        )?;
        hr(
            in_t.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC),
            "设 AAC subtype",
        )?;
        hr(
            in_t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate),
            "设采样率",
        )?;
        hr(
            in_t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels as u32),
            "设声道数",
        )?;
        // 默认值随系统版本有出入，显式钉死 raw 形态
        if let Err(e) = in_t.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0) {
            log::debug!("[Platform/mf] 设 AAC payload type 被拒（按默认）: {e}");
        }
        // MF_MT_USER_DATA 的形态是「12 字节 HEAACWAVEINFO 尾巴 + ASC」：
        // dwElementType/wPayloadType/profile/wStructType/保留位全 0 =
        // raw / 未指定，ASC 接在 12 字节之后。**裸 ASC 会被
        // MF_E_INVALIDMEDIATYPE 拒收**（Win11 26200 的 MS AAC 解码器实测，
        // 解码器自己枚举的可用类型也是 12 字节尾巴形态）。
        let user_data = [vec![0u8; 12], asc.to_vec()].concat();
        if let Err(e) = in_t.SetBlob(&MF_MT_USER_DATA, &user_data) {
            log::debug!("[Platform/mf] 设 ASC 被拒（按默认）: {e}");
        }
        hr(xform.SetInputType(0, &in_t, 0), "设音频输入类型")?;

        // 输出类型同样从解码器枚举的**可用类型**里选：手工最小类型缺
        // bits-per-sample/block-alignment，本机 MS AAC 解码器直接
        // MF_E_INVALIDMEDIATYPE（可用类型是完整形态，实测接受）。
        let mut out_set = false;
        let mut idx = 0u32;
        while let Ok(t) = xform.GetOutputAvailableType(0, idx) {
            if t.GetGUID(&MF_MT_SUBTYPE)
                .is_ok_and(|g| g == MFAudioFormat_Float)
            {
                hr(xform.SetOutputType(0, &t, 0), "设音频输出类型")?;
                out_set = true;
                break;
            }
            idx += 1;
        }
        if !out_set {
            // 兜底：手工 Float，把解码器校验的字段补全
            let out_t = hr(MFCreateMediaType(), "建音频输出类型")?;
            hr(
                out_t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio),
                "设音频 major type",
            )?;
            hr(
                out_t.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float),
                "设 f32 subtype",
            )?;
            hr(
                out_t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate),
                "设输出采样率",
            )?;
            hr(
                out_t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels as u32),
                "设输出声道数",
            )?;
            hr(
                out_t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 32),
                "设输出位深",
            )?;
            hr(
                out_t.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, u32::from(channels) * 4),
                "设输出块对齐",
            )?;
            hr(xform.SetOutputType(0, &out_t, 0), "设音频输出类型")?;
        }
        driver.begin_streaming()?;

        while let Ok(req) = rx.recv() {
            match req {
                Req::Shutdown => return Ok(()),
                Req::Flush => {
                    let _ = xform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
                    let _ = tx.send(Rsp::Samples(Vec::new()));
                }
                Req::Finish => {
                    let _ = tx.send(Rsp::Done);
                }
                Req::Decode { sample, .. } => {
                    let input = match bytes_sample(&sample, -1, -1) {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = tx.send(Rsp::Err(e));
                            continue;
                        }
                    };
                    if let Err(e) = driver.push(input) {
                        let _ = tx.send(Rsp::Err(e));
                        continue;
                    }
                    let mut pcm = Vec::new();
                    for s in driver.take() {
                        match sample_to_bytes(&s) {
                            Ok((bytes, _, _)) => pcm.extend(
                                bytes
                                    .as_chunks::<4>()
                                    .0
                                    .iter()
                                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])),
                            ),
                            Err(e) => log::warn!("[Platform/mf] 丢一段音频输出: {e}"),
                        }
                    }
                    let _ = tx.send(Rsp::Samples(pcm));
                }
            }
        }
        Ok(())
    }
}

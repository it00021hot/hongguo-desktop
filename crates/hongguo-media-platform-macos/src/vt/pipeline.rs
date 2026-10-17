//! 转码管线编排：HEVC→H.264 主流程（[`transcode_h264`] 入口、[`run`] 流水）、
//! 逐帧喂编码的 [`process_frame`] / [`emit_outputs`] 与时间换算小工具。

use std::time::Instant;

use hongguo_common::{AppError, AppResult};
use hongguo_video::sample_table::TrackInfo;

use hongguo_media_platform_api::PlatformRequest;
use hongguo_media_platform_api::video::{VideoCodec, bitrate_for};

use super::decode::Decompressor;
use super::encode::Compressor;
use super::ffi;
use super::pixel::{CmSb, Pb, Transfer};

/// 平台编码层执行 HEVC→H.264 转码；HEVC 以外的源返回 `None`
/// （留给 ffmpeg/软解——H.264 源在 ffmpeg 侧是直转，没必要绕 GPU）。
///
/// 闸门是「VT 会话能建」而不是「必须硬编」：会话有硬编走硬编，没有就落
/// Apple 内置软编会话——后者仍然显著快于 ffmpeg 与纯 Rust 软解（本机 i7
/// 实测 44.9s 集：VT 链 ≈5.9s，libx264 18.2s、纯 Rust 54s），且同样不依赖
/// 用户装任何东西。会话真建不出来（罕见）由管线落回下一条路。
pub fn transcode_h264(req: &PlatformRequest<'_>) -> Option<AppResult<()>> {
    let demuxed = hongguo_media::demux::demux_file(req.input).ok()?;
    let video = demuxed.video_track()?;
    if !hongguo_media::hevc::is_hevc(&video.info.codec) {
        return None;
    }
    Some(run(req, &demuxed, &video.info))
}

// ————————————————————————————————————————————————————————————
// 编排
// ————————————————————————————————————————————————————————————

// 码率标定 bitrate_for 收敛到 super::video（mf/vt 两侧原本逐字节相同）。

/// 一帧解码输出 →（缩放）→ 编码 → 收割产物。`pts` 是该帧与源槽位配对
/// 好的显示时间（见 run() 流水注释）。
#[expect(
    clippy::too_many_arguments,
    reason = "参数表与平台 C API（虚表/会话属性）一一对应，硬拆参数结构反而失真"
)]
fn process_frame(
    pb: Pb,
    pts: f64,
    scale_to: Option<(usize, usize)>,
    fps: f64,
    frame_dur: f64,
    transfer: &mut Option<Transfer>,
    compressor: &mut Option<Compressor>,
    out_dims: &mut Option<(usize, usize)>,
    units: &mut Vec<(f64, Vec<u8>, bool)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<()> {
    // 首帧才知道真实输出分辨率，编码会话延迟到这里建
    if compressor.is_none() {
        let (src_w, src_h) = pb.dims();
        let (out_w, out_h) = match scale_to {
            Some((w, h)) if (w, h) != (src_w, src_h) => {
                *transfer = Some(unsafe { Transfer::new(w, h)? });
                (w, h)
            }
            Some((w, h)) => (w, h),
            None => (src_w, src_h),
        };
        let c = unsafe { Compressor::new(out_w, out_h, bitrate_for(out_w, out_h, fps))? };
        // 编码器自报硬编还是软编——只进日志，产物两边都是能播的 H.264
        let hw = unsafe { c.using_hardware() }.unwrap_or(false);
        log::info!(
            "[Platform/vt] 编码会话已建（{out_w}x{out_h}）：{}",
            if hw { "硬件" } else { "Apple 软编" }
        );
        *compressor = Some(c);
        *out_dims = Some((out_w, out_h));
    }
    let frame = match transfer.as_ref() {
        Some(t) => unsafe { t.scale(&pb) }?,
        None => pb,
    };
    let compressor = compressor.as_mut().expect("上一行刚建好");
    unsafe { compressor.encode(&frame, pts, frame_dur)? };
    emit_outputs(compressor, units, on_progress)
}

/// 收割编码器已就绪的输出。
///
/// 显示时间读**输出样本自带的 PTS**（编码器保留编码时传入的时间戳），
/// 不依赖「输出序=输入序」——个别编码器拒收 MaxFrameDelayCount=0 时可能
/// 重排，arrival 序标时间会整体错位。递增兜底仍在：畸形时间按半帧顶开。
fn emit_outputs(
    compressor: &mut Compressor,
    units: &mut Vec<(f64, Vec<u8>, bool)>,
    on_progress: Option<&(dyn Fn(f64) + Send + Sync)>,
) -> AppResult<()> {
    for sb in compressor.take_outputs() {
        let (annexb, keyframe) = unsafe { compressor.sample_to_unit(&sb) }?;
        // PTS **原样透传**。解码侧按源时间轴精确配对后，发射序=解码序，
        // 显示时间天然非单调——任何「回退顶开」都会把真实显示顺序打乱
        // （真实剧集 B 帧流首跑实证：标题帧被顶进转场中间）。非单调交给
        // muxide：按时间戳交织、写 ctts，这正是 MP4 B 帧的标准形态。
        let pts = output_pts_secs(&sb);
        units.push((pts, annexb, keyframe));
        if let Some(cb) = on_progress {
            cb(pts);
        }
    }
    Ok(())
}

/// CMTime → 秒。timescale 非法时按 0 处理，交给递增兜底。
pub(super) fn cm_time_secs(t: ffi::CMTime) -> f64 {
    if t.timescale <= 0 {
        0.0
    } else {
        t.value as f64 / f64::from(t.timescale)
    }
}

/// 输出样本的显示时间（秒）。
fn output_pts_secs(sb: &CmSb) -> f64 {
    cm_time_secs(unsafe { ffi::CMSampleBufferGetOutputPresentationTimeStamp(sb.0) })
}

/// 显示时间 → 90000 刻度整数键：与 decode() 喂给 VT 的取整同一刻度，
/// 解码回调带回的值可与源槽位精确相等，用作帧—时间槽的配对键。
fn ptkey(secs: f64) -> i64 {
    (secs * 90_000.0).round() as i64
}

fn run(
    req: &PlatformRequest<'_>,
    demuxed: &hongguo_media::demux::Demuxed,
    video: &TrackInfo,
) -> AppResult<()> {
    let started = Instant::now();

    // 1) 参数集与源时间轴。hvcC 里可能混着非参数集 NAL（实测 libx265 会把
    //    两 KB 的 SEI 塞进 hvcC），格式描述只收 VPS/SPS/PPS，其余剔除。
    let (raw_sets, _) = hongguo_media::hevc::read_parameter_set_nalus(req.input, video)?;
    let parameter_sets: Vec<Vec<u8>> = raw_sets
        .into_iter()
        .filter(|n| !n.is_empty() && matches!((n[0] >> 1) & 0x3f, 32..=34))
        .collect();
    let fps = video.average_framerate().unwrap_or(25.0);
    let frame_dur = 1.0 / fps;
    // 每个样本自己的显示时间（stts 累计 + ctts 合成偏移）；空表退化为按
    // 平均帧率均摊。帧与时间槽的配对全靠它，见下面的流水注释。
    let mut source_pts = video.sample_pts();
    if source_pts.is_empty() {
        source_pts = (0..video.samples.len()).map(|i| i as f64 / fps).collect();
    }
    let scale_to = req
        .scale_to
        .map(|(w, h)| ((w & !1) as usize, (h & !1) as usize));

    // 2) 解码 → （缩放）→ 编码 流水。两个「顺序不可信」，各修各的：
    //
    // - **解码吐出序不可信**：同步解码（回调在 decode() 内触发）没有
    //   B 帧重排，吐出的是解码序；解码回调把喂入的显示时间原样带回，
    //   帧↔显示时间按 PTS 认领，绝不按吐出顺序排位。
    // - **编码器输入必须是显示序**：编码器契约按显示序收帧、自行构造
    //   B 帧 GOP 并写 POC。把解码序直接喂进去，POC 会把解码序固化成
    //   显示序——产物按解码序播放（真实剧集实证：标题帧闪现在转场前，
    //   showinfo 呈现序 0, 0.533, 0.067…）。配对好的帧按容器时间槽顺序
    //   喂编码器；槽内帧未到就继续解码下一个样本，内存以重排窗口为界。
    // 转码只处理 HEVC 源（H.264 源在 ffmpeg 侧直转），解码会话固定按 HEVC 建
    let decoder = unsafe { Decompressor::new(VideoCodec::Hevc, &parameter_sets, false)? };
    let mut slot_times = source_pts.clone();
    slot_times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut ready: Vec<(Pb, f64)> = Vec::new();
    let mut units: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    let mut transfer: Option<Transfer> = None;
    let mut compressor: Option<Compressor> = None;
    let mut out_dims: Option<(usize, usize)> = None;
    let mut slot = 0usize;

    let feed_ready_slots = |ready: &mut Vec<(Pb, f64)>,
                            slot: &mut usize,
                            transfer: &mut Option<Transfer>,
                            compressor: &mut Option<Compressor>,
                            out_dims: &mut Option<(usize, usize)>,
                            units: &mut Vec<(f64, Vec<u8>, bool)>|
     -> AppResult<()> {
        while *slot < slot_times.len() {
            let key = ptkey(slot_times[*slot]);
            let Some(pos) = ready.iter().position(|(_, k)| ptkey(*k) == key) else {
                break;
            };
            let (pb, pts) = ready.swap_remove(pos);
            process_frame(
                pb,
                pts,
                scale_to,
                fps,
                frame_dur,
                transfer,
                compressor,
                out_dims,
                units,
                req.on_progress,
            )?;
            *slot += 1;
        }
        Ok(())
    };

    for (i, &(offset, size)) in video.samples.iter().enumerate() {
        let raw = hongguo_media::transcode::read_range(req.input, offset, size)?;
        if raw.is_empty() {
            continue;
        }
        let pts = source_pts.get(i).copied().unwrap_or(0.0);
        unsafe { decoder.decode(&raw, pts, frame_dur)? };
        decoder.drain(&mut ready);
        feed_ready_slots(
            &mut ready,
            &mut slot,
            &mut transfer,
            &mut compressor,
            &mut out_dims,
            &mut units,
        )?;
    }
    unsafe { decoder.finish()? };
    // 收尾：同步解码逐帧结清，此处正常应为空；万一有遗留，按时间槽补齐
    decoder.drain(&mut ready);
    feed_ready_slots(
        &mut ready,
        &mut slot,
        &mut transfer,
        &mut compressor,
        &mut out_dims,
        &mut units,
    )?;
    // 时间槽全过完后仍剩余的帧（源时间轴之外的重复/畸形）——按 PTS 排序追加
    ready.sort_by_key(|(_, pts)| ptkey(*pts));
    for (pb, pts) in ready {
        process_frame(
            pb,
            pts,
            scale_to,
            fps,
            frame_dur,
            &mut transfer,
            &mut compressor,
            &mut out_dims,
            &mut units,
            req.on_progress,
        )?;
    }

    // 3) 冲出编码器尾帧
    let mut compressor = match compressor {
        Some(c) => c,
        None => return Err(AppError::Media("HEVC 解码没有产出任何帧".into())),
    };
    unsafe { compressor.finish()? };
    emit_outputs(&mut compressor, &mut units, req.on_progress)?;

    if units.is_empty() {
        return Err(AppError::Media("平台硬编没有产出任何帧".into()));
    }

    // 4) 音轨直通 + 封装（与软解路径同一套代码，faststart）
    let (out_w, out_h) = out_dims.unwrap_or((video.width as usize, video.height as usize));
    hongguo_media::transcode::mux_with_audio(
        req.input, demuxed, req.output, &units, out_w, out_h, fps as f32,
    )?;

    log::info!(
        "[Platform/vt] HEVC→H.264 完成：{} 帧（{out_w}x{out_h}），耗时 {:?}",
        units.len(),
        started.elapsed()
    );
    Ok(())
}

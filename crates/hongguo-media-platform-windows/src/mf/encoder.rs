//! 编码：H.264 MFT（硬件优先，无硬编回落 in-box 软件），跑在 [`super::mft::Driver`]
//! 的统一驱动之上；参数集的来源与码流形态归一见 [`super::annexb`]。

use std::collections::VecDeque;

use windows::Win32::Media::MediaFoundation::*;

use hongguo_common::{AppError, AppResult};

use super::annexb::{param_sets_annexb_or_none, read_type_param_sets};
use super::decoder::Nv12Frame;
use super::mft::{Driver, enum_activates, hr, set_codecapi_u32, type_info};
use super::sample::{
    nv12_sample, sample_is_clean, sample_to_bytes, secs_to_hns, set_size_rate, type_stride,
    video_type,
};

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum EncoderKind {
    /// 硬件 MFT（`MFT_ENUM_FLAG_HARDWARE`），探测验证过才用
    Hardware,
    /// in-box 软件编码器（h264_mf，同步 MFT）——无硬编时正式分流回落
    Software,
}

pub(super) struct Encoder {
    driver: Driver,
    stride: usize,
    /// SPS/PPS（Annex-B），关键帧前插用（拼接器重建 moov 时样本描述取自第 1 集，后续各集码流必须自带参数集）。
    /// 编码器把它放在输出类型的 `MF_MT_MPEG_SEQUENCE_HEADER` 或首个输出样本里，两处都试。
    param_sets: Option<Vec<u8>>,
    out_type: IMFMediaType,
    /// 每次喂帧记录的显示时间（秒），输出按到达顺序配对取用——实测 h264_mf 无视输入时间戳、按自己的 30fps 假设重打输出时间，NVENC 则原样保留；不信任回传值，统一用喂入值。
    fed_pts: VecDeque<f64>,
}

impl Encoder {
    pub(super) fn new(
        width: usize,
        height: usize,
        fps_num: u32,
        fps_den: u32,
        bitrate: i32,
        kind: EncoderKind,
    ) -> AppResult<Self> {
        let flags = match kind {
            EncoderKind::Hardware => {
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_ASYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER
            }
            // 没有 SOFTWARE 标志：软件 MFT = SYNCMFT（不带 HARDWARE）
            EncoderKind::Software => MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
        };
        let activaes = enum_activates(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            Some(&type_info(MFMediaType_Video, MFVideoFormat_NV12)),
            Some(&type_info(MFMediaType_Video, MFVideoFormat_H264)),
        )?;
        let label = match kind {
            EncoderKind::Hardware => "硬件",
            EncoderKind::Software => "软件",
        };
        let mut last = AppError::Media(format!("没有可用的{label} H.264 编码 MFT"));
        for a in &activaes {
            match Self::try_build(a, width, height, fps_num, fps_den, bitrate) {
                Ok(e) => return Ok(e),
                Err(e) => {
                    log::debug!("[Platform/mf] {label}编码 MFT 不可用，试下一个: {e}");
                    last = e;
                }
            }
        }
        Err(last)
    }

    fn try_build(
        a: &IMFActivate,
        width: usize,
        height: usize,
        fps_num: u32,
        fps_den: u32,
        bitrate: i32,
    ) -> AppResult<Encoder> {
        // 类型协商顺序：硬件编码 MFT 普遍要求**先设输出类型再设输入**（NVIDIA 的 MFT 先设输入直接回 MF_E_INVALIDTYPE 0xC00D6D60），
        // in-box 软件编码器两种顺序都收——两种顺序各试一遍
        let mut last = AppError::Media("编码 MFT 类型协商失败".into());
        for output_first in [true, false] {
            match Self::negotiate_types(a, width, height, fps_num, fps_den, bitrate, output_first) {
                Ok(e) => return Ok(e),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn negotiate_types(
        a: &IMFActivate,
        width: usize,
        height: usize,
        fps_num: u32,
        fps_den: u32,
        bitrate: i32,
        output_first: bool,
    ) -> AppResult<Encoder> {
        let (mut driver, xform) = Driver::new(a)?;
        let build_input = |xform: &IMFTransform| -> AppResult<()> {
            unsafe {
                // 输入类型：优先用 MFT 列出的 NV12 模板（保住它自带的属性），帧尺寸/帧率/行距由我们覆盖；列不出再自建
                let mut offered = None;
                let mut i = 0u32;
                loop {
                    let t = match xform.GetInputAvailableType(0, i) {
                        Ok(t) => t,
                        Err(_) => break,
                    };
                    if t.GetGUID(&MF_MT_SUBTYPE).map(|g| g == MFVideoFormat_NV12) == Ok(true) {
                        offered = Some(t);
                        break;
                    }
                    i += 1;
                }
                let t = match offered {
                    Some(offered) => {
                        let fresh = hr(MFCreateMediaType(), "建媒体类型")?;
                        hr(offered.CopyAllItems(&fresh), "拷贝输入类型")?;
                        fresh
                    }
                    None => video_type(MFVideoFormat_NV12)?,
                };
                set_size_rate(&t, width as u32, height as u32, fps_num, fps_den)?;
                let _ = t.SetUINT32(&MF_MT_DEFAULT_STRIDE, width as u32);
                hr(xform.SetInputType(0, &t, 0), "设编码输入类型")
            }
        };
        let build_output = |xform: &IMFTransform| -> AppResult<()> {
            // 码控走 ICodecAPI（对齐 ffmpeg mfenc 的配置面）：不设的话 h264_mf 处于不可用的默认码控状态——
            // 实测只吐 SPS/PPS/AUD、一帧 slice 都不编。全部尽力而为，硬要求仍只有媒体类型上的码率
            set_codecapi_u32(
                xform,
                &CODECAPI_AVEncCommonRateControlMode,
                eAVEncCommonRateControlMode_CBR.0 as u32,
                "码控模式 CBR",
            );
            set_codecapi_u32(
                xform,
                &CODECAPI_AVEncCommonMeanBitRate,
                bitrate.max(1) as u32,
                "均值码率",
            );
            set_codecapi_u32(xform, &CODECAPI_AVEncMPVGOPSize, 250, "GOP");
            set_codecapi_u32(xform, &CODECAPI_AVEncMPVDefaultBPictureCount, 0, "关 B 帧");
            set_codecapi_u32(xform, &CODECAPI_AVEncCommonRealTime, 1, "实时码控");
            unsafe {
                // 输出类型：H.264 + 码率。只有码率是硬性要求（码控失效=质量目标失效，对齐 vt 侧的分档）；profile 与 GOP 间隔尽力而为
                let o = video_type(MFVideoFormat_H264)?;
                set_size_rate(&o, width as u32, height as u32, fps_num, fps_den)?;
                hr(
                    o.SetUINT32(&MF_MT_AVG_BITRATE, bitrate.max(1) as u32),
                    "设码率",
                )?;
                let _ = o.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32);
                let _ = o.SetUINT32(&MF_MT_MAX_KEYFRAME_SPACING, 250);
                hr(xform.SetOutputType(0, &o, 0), "设编码输出类型")
            }
        };
        if output_first {
            build_output(&xform)?;
            build_input(&xform)?;
        } else {
            build_input(&xform)?;
            build_output(&xform)?;
        }
        driver.begin_streaming()?;
        let in_cur = hr(unsafe { xform.GetInputCurrentType(0) }, "读编码输入类型")?;
        let stride = type_stride(&in_cur, width);
        let out_type = hr(unsafe { xform.GetOutputCurrentType(0) }, "读编码输出类型")?;
        let param_sets = read_type_param_sets(&out_type);
        Ok(Encoder {
            driver,
            stride,
            param_sets,
            out_type,
            fed_pts: VecDeque::new(),
        })
    }

    /// 编一帧（显示时间戳由调用方按源时间轴给）。
    pub(super) fn encode(
        &mut self,
        frame: Nv12Frame,
        pts_secs: f64,
        dur_secs: f64,
    ) -> AppResult<()> {
        self.fed_pts.push_back(pts_secs);
        let sample = nv12_sample(
            &frame.data,
            frame.stride,
            frame.width,
            frame.height,
            self.stride,
            secs_to_hns(pts_secs),
            secs_to_hns(dur_secs),
        )?;
        self.driver.push(sample)
    }

    /// 收尾并冲出尾帧。
    pub(super) fn finish(&mut self) -> AppResult<()> {
        self.driver.finish()
    }

    /// 收割编码输出（Annex-B 单元 + 是否关键帧 + 显示时间）。
    /// 显示时间取喂帧时记录的时间轴（按到达顺序配对）；配不上（异常多出的输出）给 NaN，由上层递增兜底顶开。
    pub(super) fn take_units(&mut self) -> AppResult<Vec<(f64, Vec<u8>, bool)>> {
        self.driver.harvest()?;
        let samples = self.driver.take();
        let mut units = Vec::with_capacity(samples.len());
        for s in samples {
            let (raw, t, _d) = sample_to_bytes(&s)?;
            let keyframe = sample_is_clean(&s);
            if self.param_sets.is_none() {
                // 类型里没有就重读一次（有的 MFT 首样本后才填序列头），再不行从这个样本里解析（两处形态都见过）
                let mut ps = read_type_param_sets(&self.out_type);
                if ps.is_none() {
                    ps = param_sets_annexb_or_none(&raw);
                }
                self.param_sets = ps;
            }
            // 编码器输出格式实测不统一：h264_mf 直接吐 Annex-B（起始码开头），别的可能给 AVCC——按单元嗅探，
            // 别把 Annex-B 当 AVCC 转换（长度字段走读会错读起始码，把 slice 整段丢掉）
            let mut annexb = if raw.starts_with(&[0, 0, 0, 1]) || raw.starts_with(&[0, 0, 1]) {
                raw.clone()
            } else {
                hongguo_media::hevc::to_annexb(&raw, 4)
            };
            // 关键帧（以及首个样本，无论标没标）前插 SPS/PPS
            let first = units.is_empty();
            if (keyframe || first)
                && let Some(ps) = self.param_sets.as_deref()
                && !ps.is_empty()
            {
                annexb.splice(0..0, ps.iter().copied());
            }
            let _ = t;
            let pts = self.fed_pts.pop_front().unwrap_or(f64::NAN);
            units.push((pts, annexb, keyframe));
        }
        Ok(units)
    }
}

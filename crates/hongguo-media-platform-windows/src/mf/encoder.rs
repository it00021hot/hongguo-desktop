//! NV12 → HEVC；MFT 与 COM 对象由调用方的 MF 线程独占。

use std::collections::VecDeque;

use windows::Win32::Media::MediaFoundation::*;

use hongguo_common::{AppError, AppResult};

use super::annexb::{normalize, param_sets_annexb_or_none, read_type_param_sets};
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
    /// 实际可用的软件 HEVC MFT。
    Software,
}

pub(super) struct Encoder {
    driver: Driver,
    stride: usize,
    /// 每个关键帧携带完整 VPS/SPS/PPS。
    param_sets: Option<Vec<u8>>,
    out_type: IMFMediaType,
    /// 无重排输出按输入时间轴配对。
    fed_pts: VecDeque<f64>,
    emitted: bool,
    pending_headers: Vec<u8>,
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
            Some(&type_info(MFMediaType_Video, MFVideoFormat_HEVC)),
        )?;
        let label = match kind {
            EncoderKind::Hardware => "硬件",
            EncoderKind::Software => "软件",
        };
        let mut last = AppError::Media(format!("没有可用的{label} HEVC 编码 MFT"));
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
        Self::negotiate_types(a, width, height, fps_num, fps_den, bitrate)
    }

    fn negotiate_types(
        a: &IMFActivate,
        width: usize,
        height: usize,
        fps_num: u32,
        fps_den: u32,
        bitrate: i32,
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
                let o = video_type(MFVideoFormat_HEVC)?;
                set_size_rate(&o, width as u32, height as u32, fps_num, fps_den)?;
                hr(
                    o.SetUINT32(&MF_MT_AVG_BITRATE, bitrate.max(1) as u32),
                    "设码率",
                )?;
                hr(
                    o.SetUINT32(&MF_MT_VIDEO_PROFILE, eAVEncH265VProfile_Main_420_8.0 as u32),
                    "设 HEVC Main profile",
                )?;
                let _ = o.SetUINT32(&MF_MT_MAX_KEYFRAME_SPACING, 250);
                hr(xform.SetOutputType(0, &o, 0), "设编码输出类型")
            }
        };
        build_output(&xform)?;
        build_input(&xform)?;
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
            emitted: false,
            pending_headers: Vec::new(),
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
        if let Some(out_type) = self.driver.take_changed_output_type() {
            self.param_sets = read_type_param_sets(&out_type);
            self.out_type = out_type;
        }
        let samples = self.driver.take();
        let mut units = Vec::with_capacity(samples.len());
        for s in samples {
            let (raw, t, _d) = sample_to_bytes(&s)?;
            let clean = sample_is_clean(&s);
            let (mut annexb, has_video, irap) = normalize(&raw)?;
            if self.param_sets.is_none() {
                // 类型里没有就重读一次（有的 MFT 首样本后才填序列头），再不行从这个样本里解析（两处形态都见过）
                let mut ps = read_type_param_sets(&self.out_type);
                if ps.is_none() {
                    self.pending_headers.extend_from_slice(&annexb);
                    if self.pending_headers.len() > 1024 * 1024 {
                        return Err(AppError::Media("HEVC 编码参数集超过限制".into()));
                    }
                    ps = param_sets_annexb_or_none(&self.pending_headers);
                }
                self.param_sets = ps;
                if self.param_sets.is_some() {
                    self.pending_headers.clear();
                }
            }
            if !has_video {
                continue;
            }
            let keyframe = clean || irap;
            if !self.emitted && !keyframe {
                return Err(AppError::Media("HEVC 编码首帧不是随机访问帧".into()));
            }
            if keyframe {
                let ps = self
                    .param_sets
                    .as_deref()
                    .ok_or_else(|| AppError::Media("HEVC 编码输出缺少完整 VPS/SPS/PPS".into()))?;
                annexb.splice(0..0, ps.iter().copied());
            }
            let pts = self
                .fed_pts
                .pop_front()
                .ok_or_else(|| AppError::Media("HEVC 编码器返回了未提交的帧".into()))?;
            if t < 0 || t.abs_diff(secs_to_hns(pts)) > 1 {
                return Err(AppError::Media(
                    "HEVC 编码器重排或改写了输入显示时间".into(),
                ));
            }
            units.push((pts, annexb, keyframe));
            self.emitted = true;
        }
        Ok(units)
    }
}

//! 缩放：Video Processor MFT（同步、NV12→NV12），可选环节——统一输出
//! 分辨率与源不一致时由 pipeline 建会话接在解码之后。

use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};

use hongguo_common::{AppError, AppResult};

use super::decoder::Nv12Frame;
use super::mft::{Pulled, SyncMft, hr};
use super::sample::{nv12_sample, sample_to_bytes, set_size_rate, type_stride, video_type};

pub(super) struct Scaler {
    mft: SyncMft,
    width: usize,
    height: usize,
    stride: usize,
}

impl Scaler {
    pub(super) fn new(
        src_w: usize,
        src_h: usize,
        src_stride: usize,
        dst_w: usize,
        dst_h: usize,
        fps_num: u32,
        fps_den: u32,
    ) -> AppResult<Self> {
        unsafe {
            let xform: IMFTransform = hr(
                CoCreateInstance(&CLSID_VideoProcessorMFT, None, CLSCTX_ALL),
                "建 Video Processor",
            )?;
            let in_t = video_type(MFVideoFormat_NV12)?;
            set_size_rate(&in_t, src_w as u32, src_h as u32, fps_num, fps_den)?;
            hr(
                in_t.SetUINT32(&MF_MT_DEFAULT_STRIDE, src_stride as u32),
                "设输入行距",
            )?;
            hr(xform.SetInputType(0, &in_t, 0), "设缩放输入类型")?;
            let out_t = video_type(MFVideoFormat_NV12)?;
            set_size_rate(&out_t, dst_w as u32, dst_h as u32, fps_num, fps_den)?;
            hr(xform.SetOutputType(0, &out_t, 0), "设缩放输出类型")?;
            let mut mft = SyncMft {
                xform,
                provides_samples: false,
                out_size: 0,
            };
            mft.refresh_out_info()?;
            hr(
                mft.xform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0),
                "开始缩放流",
            )?;
            let cur = hr(mft.xform.GetOutputCurrentType(0), "读缩放输出类型")?;
            let stride = type_stride(&cur, dst_w);
            Ok(Scaler {
                mft,
                width: dst_w,
                height: dst_h,
                stride,
            })
        }
    }

    pub(super) fn scale(&mut self, frame: Nv12Frame) -> AppResult<Nv12Frame> {
        let sample = nv12_sample(
            &frame.data,
            frame.stride,
            frame.width,
            frame.height,
            frame.stride,
            frame.pts_100ns,
            frame.dur_100ns,
        )?;
        unsafe {
            hr(self.mft.xform.ProcessInput(0, &sample, 0), "缩放进帧")?;
        }
        let mut out = Vec::new();
        loop {
            match self.mft.pull_once()? {
                Pulled::Sample(s) => out.push(s),
                Pulled::NeedMoreInput => break,
                Pulled::StreamChange => {
                    self.mft.refresh_out_info()?;
                }
            }
        }
        let mut frames = out
            .into_iter()
            .filter_map(|s| {
                let (data, t, d) = sample_to_bytes(&s).ok()?;
                Some(Nv12Frame {
                    data,
                    stride: self.stride,
                    width: self.width,
                    height: self.height,
                    pts_100ns: t,
                    dur_100ns: d,
                })
            })
            .collect::<Vec<_>>();
        match frames.pop() {
            Some(f) => Ok(f),
            None => Err(AppError::Media("Video Processor 没有产出缩放帧".into())),
        }
    }
}

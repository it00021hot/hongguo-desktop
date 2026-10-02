//! HEVC 解码 → YUV，YUV → H.264 编码。
//!
//! 与 `transcode.rs`（编排）和 `transcode/mux.rs`（封装）分文件：
//! 三步各占一个文件，才看得出转码到底卡在哪一步。
//!
//! ⚠️ 这一层是**流水线**而不是「先全解码、再全编码」：
//! 解码器每吐一帧就立刻喂给编码器，随即释放该帧的 YUV。
//! 整集帧攒在 `Vec` 里的写法（1080p 一集几百帧 = 6~9 GB）会直接 OOM，
//! 而流水线下峰值只与**解码器的帧重排深度**有关，与片长无关。

use std::path::Path;

use crate::error::{AppError, AppResult};

use super::{read_range, TranscodeOptions, YuvFrame};

/// 流水线产物：H.264 访问单元序列 + 视频参数。
pub(super) struct Encoded {
    /// `(时间戳秒, 访问单元字节, 是否关键帧)`
    pub units: Vec<(f64, Vec<u8>, bool)>,
    pub width: usize,
    pub height: usize,
    /// 实际喂进编码器的帧数
    pub frames: usize,
}

/// HEVC 解码 → 立即 H.264 编码。
///
/// 逐样本喂给解码器，每解出一帧就编码并丢弃其 YUV，
/// 所以内存占用是「解码器内部缓冲 + 一帧」而不是「整集」。
///
/// `lookahead` 已关（见 [`encode_h264`]），编码器不会额外缓存 GOP；
/// 保留它是为了保住 B 帧质量，代价是解码器需要多解几帧才能吐出第一帧。
pub(super) fn decode_and_encode(
    input: &Path,
    prefix: &[u8],
    length_size: usize,
    samples: &[(u64, u64)],
    options: &TranscodeOptions,
) -> AppResult<Encoded> {
    use rusty_h265::Decoder;

    let mut decoder = Decoder::new();
    if !prefix.is_empty() {
        decoder
            .push_annexb(prefix, None)
            .map_err(|e| AppError::Media(format!("HEVC 参数集解码失败: {e}")))?;
    }

    // 编码器要等第一帧出来才知道分辨率，所以延迟到这里再构造
    let mut encoder: Option<EncoderState> = None;
    let mut units: Vec<(f64, Vec<u8>, bool)> = Vec::new();
    let mut width = 0usize;
    let mut height = 0usize;
    let mut frame_index = 0usize;

    // 把解码器当前可输出的帧全部喂进编码器。
    // `next_frame()` 返回 Err::Again 只是「暂时没有更多帧」，不是错误。
    let drain = |decoder: &mut Decoder,
                 encoder: &mut Option<EncoderState>,
                 units: &mut Vec<(f64, Vec<u8>, bool)>,
                 width: &mut usize,
                 height: &mut usize,
                 frame_index: &mut usize|
     -> AppResult<()> {
        while let Ok(frame) = decoder.next_frame() {
            if *width == 0 {
                *width = frame.width;
                *height = frame.height;
            }
            let yuv = extract_yuv(&frame);
            if encoder.is_none() {
                *encoder = Some(EncoderState::new(*width, *height, options)?);
            }
            let enc = encoder.as_mut().expect("上一行刚建好");
            enc.push(&yuv, *frame_index, units);
            *frame_index += 1;
            // yuv 不出这个作用域就释放，不会在 Vec 里累积——这正是流水的意义
        }
        Ok(())
    };

    for (i, &(offset, size)) in samples.iter().enumerate() {
        let raw = read_range(input, offset, size)?;
        if raw.is_empty() {
            continue;
        }
        // MP4 样本是「长度前缀 + NAL」，解码器只认 Annex-B
        let data = crate::media::hevc::to_annexb(&raw, length_size);
        if data.is_empty() {
            continue;
        }

        decoder
            .push_annexb(&data, Some(i as i64))
            .map_err(|e| AppError::Media(format!("样本 {i} 解码失败: {e}")))?;

        drain(
            &mut decoder,
            &mut encoder,
            &mut units,
            &mut width,
            &mut height,
            &mut frame_index,
        )?;
    }

    decoder.flush();
    drain(
        &mut decoder,
        &mut encoder,
        &mut units,
        &mut width,
        &mut height,
        &mut frame_index,
    )?;

    if width == 0 || height == 0 {
        return Err(AppError::Media("无法确定视频分辨率".into()));
    }

    let mut encoder = encoder.ok_or_else(|| AppError::Media("HEVC 解码没有产出任何帧".into()))?;
    encoder.finish(&mut units);

    if units.is_empty() {
        return Err(AppError::Media("H.264 编码没有产出任何帧".into()));
    }

    Ok(Encoded {
        units,
        width,
        height,
        frames: frame_index,
    })
}

/// 编码器状态。抽成 struct 是为了能在闭包里用 `&mut` 借用，
/// 免得把 `Encoder`（非 Copy）搬来搬去。
struct EncoderState {
    inner: rusty_h264::Encoder,
    width: usize,
    height: usize,
    framerate: f32,
    gop: u64,
}

impl EncoderState {
    fn new(width: usize, height: usize, options: &TranscodeOptions) -> AppResult<Self> {
        use rusty_h264::{Encoder, EncoderConfig};

        // 编码器要求宽高为偶数
        let w = width & !1;
        let h = height & !1;
        let mut cfg = EncoderConfig::new(w, h);
        cfg.qp = options.clamped_qp();
        cfg.gop_size = options.gop_size.max(1);
        cfg.framerate = options.framerate.max(1.0);
        // 关掉 mb-tree 前瞻：默认 40 帧缓冲会让前几帧 encode() 返回空，
        // 必须等 flush 才吐数据。逐帧处理场景不需要前瞻，关掉更简单也更快。
        cfg.lookahead = 0;
        cfg.scenecut = 0;
        // 单参考帧：编码占整条流水线 89%，而多参考帧（默认 3）对短剧这种
        // 镜头运动不大的内容几乎没有收益。实测 1080p 编码 19.0 → 24.6 fps
        //（1.30x），码流还小了 1%——两个方向都不亏。
        cfg.num_ref_frames = 1;

        // framerate 在 Encoder::new(cfg) 之前取出来：new 消耗 cfg，之后就拿不到了
        let framerate = cfg.framerate;
        let inner = Encoder::new(cfg)
            .map_err(|e| AppError::Media(format!("H.264 编码器初始化失败: {e}")))?;
        Ok(Self {
            inner,
            width: w,
            height: h,
            framerate,
            gop: u64::from(options.gop_size.max(1)),
        })
    }

    /// 喂一帧。用 `encode_planes` 直接借平面，避开 `YuvFrame` 的整帧复制。
    fn push(&mut self, yuv: &YuvFrame, index: usize, units: &mut Vec<(f64, Vec<u8>, bool)>) {
        let (y, u, v, _sy, _sc) = yuv;
        let (w, h) = (self.width, self.height);
        // 尺寸与编码器不匹配时（解码分辨率变化）跳过并记日志
        if y.len() < w * h || u.len() < (w / 2) * (h / 2) || v.len() < (w / 2) * (h / 2) {
            log::warn!("[Transcode] 第 {index} 帧尺寸不匹配，跳过");
            return;
        }

        // extract_yuv 产出的是紧凑排列（每行恰好一个行宽），所以 stride 就是宽/半宽。
        // 传错 stride 会让编码器按错误的行偏移读，越界还不报错。
        let planes = rusty_h264::YuvPlanes {
            width: w,
            height: h,
            y: &y[..w * h],
            u: &u[..(w / 2) * (h / 2)],
            v: &v[..(w / 2) * (h / 2)],
            stride_y: w,
            stride_c: w / 2,
        };
        let pts = index as f64 / f64::from(self.framerate);
        let au = self.inner.encode_planes(&planes).unwrap_or_default();
        if !au.is_empty() {
            let is_keyframe = index == 0 || (index as u64).is_multiple_of(self.gop);
            units.push((pts, au, is_keyframe));
        }
    }

    /// 排空编码器内部缓冲的尾帧。
    fn finish(&mut self, units: &mut Vec<(f64, Vec<u8>, bool)>) {
        let tail = self.inner.flush();
        if !tail.is_empty() {
            let pts = units.len() as f64 / f64::from(self.framerate);
            units.push((pts, tail, false));
        }
    }
}

/// 从解码帧取出 YUV 4:2:0 8-bit（按裁剪窗口取）。
///
/// 短剧平台是 Main profile 8-bit 4:2:0，直接取低字节；
/// 10-bit 需要右移 2 位。
fn extract_yuv(frame: &rusty_h265::Frame) -> YuvFrame {
    let p = &frame.picture;
    let (crop_x, crop_y, crop_w, crop_h) = p.crop;
    let shift = p.bit_depth_luma.saturating_sub(8);

    let w = crop_w;
    let h = crop_h;
    let cw = w / 2;
    let ch = h / 2;

    let mut y = vec![0u8; w * h];
    for row in 0..h {
        let src = p.planes[0].row(crop_y + row);
        let dst = &mut y[row * w..row * w + w];
        for (i, px) in dst.iter_mut().enumerate() {
            *px = (src[crop_x + i] >> shift) as u8;
        }
    }

    let mut u = vec![0u8; cw * ch];
    let mut v = vec![0u8; cw * ch];
    for row in 0..ch {
        let su = p.planes[1].row(crop_y / 2 + row);
        let sv = p.planes[2].row(crop_y / 2 + row);
        let du = &mut u[row * cw..row * cw + cw];
        let dv = &mut v[row * cw..row * cw + cw];
        for i in 0..cw {
            du[i] = (su[crop_x / 2 + i] >> shift) as u8;
            dv[i] = (sv[crop_x / 2 + i] >> shift) as u8;
        }
    }

    (y, u, v, w, cw)
}

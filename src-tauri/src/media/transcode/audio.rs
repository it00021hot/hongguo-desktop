//! 音轨参数读取：`esds` → 采样率 / 声道数。
//!
//! 源文件的音轨在解密后已是**明文 AAC**，可以直接打包进产物，
//! 不需要解码成 PCM 再重编码——省掉一整轮编解码，也就没有音质损失。
//! 所以这里只需求出 AAC 封装 MP4 时必须声明的两个参数。

use std::path::Path;

use crate::domain::mp4::r#box::find_box;
use crate::domain::mp4::sample_table::TrackInfo;
use crate::error::{AppError, AppResult};

/// 音轨的封装参数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioFormat {
    /// 采样率，如 44100
    pub sample_rate: u32,
    /// 声道数
    pub channels: u16,
}

/// 从轨道的 `stsd` 里读出 AAC 参数。
///
/// 取不到时回落到 AAC 最常见的 44100Hz 立体声——源片源几乎都是这个规格，
/// 猜错最多让播放器按默认参数解析，不至于整条轨播不出来。
///
/// 自己开文件只读头部（`stbl` 恒在 moov 里，位置靠前）：
/// 传整个文件进来会让调用方为了几个字节把上百 MB 的视频读进内存。
pub fn read_audio_format(path: &Path, track: &TrackInfo) -> AppResult<AudioFormat> {
    let need = track.stbl_offset + track.stbl_size;
    let len = std::fs::metadata(path)
        .map_err(|e| AppError::Io(e.to_string()))?
        .len() as usize;
    let head_len = need.min(len).min(8 * 1024 * 1024);
    let mut data = vec![0u8; head_len];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
        f.read_exact(&mut data)
            .map_err(|e| AppError::Io(e.to_string()))?;
    }
    read_audio_format_from(&data, track)
}

/// 从已读入的字节里解析 AAC 参数（`read_audio_format` 的纯函数部分）。
fn read_audio_format_from(data: &[u8], track: &TrackInfo) -> AppResult<AudioFormat> {
    let stsd = find_box(
        data,
        track.stbl_offset,
        track.stbl_offset + track.stbl_size,
        "stsd",
    )
    .ok_or_else(|| AppError::Media("音轨缺少 stsd".into()))?;
    let entry_start = stsd.start + 8;
    if entry_start + 8 > data.len() {
        return Ok(AudioFormat::default());
    }

    // sample entry 头：6B reserved + 2B data_ref_idx = 8B
    let inner = entry_start + 8;
    let entry_end = stsd.start + stsd.size;

    // esds 是 sample entry 的子 box；AudioSampleEntry 的固定字段到 inner 之前
    for candidate in [inner, inner + 20] {
        if candidate + 8 > entry_end {
            continue;
        }
        if let Some(esds) = find_box(data, candidate, entry_end, "esds")
            && let Some(fmt) = parse_esds(data, esds.start, esds.size)
        {
            return Ok(fmt);
        }
    }

    Ok(AudioFormat::default())
}

/// 解析 `esds` 里的 DecoderSpecificInfo（AudioSpecificConfig）。
///
/// ASC 前 5 位是采样率索引，第 8 位之后是声道数：
/// `sample_rate = RATES[idx]`、`channels = ((config[2] & 0x0F) << 2) | (config[2] >> 6)`。
const AAC_RATES: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];

fn parse_esds(data: &[u8], start: usize, size: usize) -> Option<AudioFormat> {
    // esds 是 FullBox，前 4 字节 version+flags
    let mut p = start + 4;
    let end = start + size;

    // ES_Descriptor: tag(1) length(1..4, 变长) ES_ID(2) flags(1)
    while p < end {
        let tag = data[p];
        p += 1;
        let (len, used) = read_descriptor_len(data, p)?;
        p += used;
        let body_end = (p + len).min(end);

        match tag {
            // DecoderConfigDescriptor: objectType(1) streamType(1) bufferSize(3)
            //   maxBitrate(4) avgBitrate(4) = 13 字节，然后 DecoderSpecificInfo
            0x04 => {
                if p + 13 > end {
                    return None;
                }
                let dsi_tag = data[p + 13];
                if dsi_tag == 0x05 {
                    let dsi_len = data[p + 14] as usize;
                    let cfg_start = p + 15;
                    let cfg = data.get(cfg_start..cfg_start + dsi_len.min(2))?;
                    return Some(audio_format_from_asc(cfg));
                }
                p = body_end;
            }
            // SLConfigDescriptor 等，直接跳过
            0x06 => p = body_end,
            _ => p = body_end,
        }
    }
    None
}

/// MPEG-4 描述符长度：每字节高位是续位，低 7 位是长度。
fn read_descriptor_len(data: &[u8], p: usize) -> Option<(usize, usize)> {
    let mut len = 0usize;
    let mut used = 0usize;
    for _ in 0..4 {
        let b = *data.get(p + used)?;
        used += 1;
        len = (len << 7) | (b & 0x7f) as usize;
        if b & 0x80 == 0 {
            return Some((len, used));
        }
    }
    None
}

fn audio_format_from_asc(cfg: &[u8]) -> AudioFormat {
    // AudioSpecificConfig（ISO/IEC 14496-3）按位流连续排布：
    //   audioObjectType        5 bit → byte0 bit7..bit3
    //   samplingFrequencyIndex 4 bit → byte0 bit2..bit0 + byte1 bit7
    //   channelConfiguration   4 bit → byte1 bit6..bit4 + byte2 bit7
    // 这两个字段都**跨字节**，按整字节取会整体错位，表现为音轨变调或无声。
    let idx = (((cfg[0] & 0x07) << 1) | (cfg[1] >> 7)) as usize;
    let sample_rate = AAC_RATES.get(idx).copied().unwrap_or(44100);
    let channels = if cfg.len() < 3 {
        2
    } else {
        ((((cfg[1] & 0x70) >> 3) | (cfg[2] >> 7)) as u16).clamp(1, 8)
    };
    AudioFormat {
        sample_rate,
        channels,
    }
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self {
            sample_rate: 44100,
            channels: 2,
        }
    }
}

/// ADTS 头长度（无 CRC）。
const ADTS_HEADER: usize = 7;

/// 给一个裸 AAC 帧套上 ADTS 头。
///
/// MP4 里的 AAC 样本是**不带帧头**的裸数据，而 muxide 的 `write_audio`
/// 期望 ADTS framing。少这一步封出来的文件能播容器、但播放器解不出音频。
pub(super) fn adts_frame(frame: &[u8], sample_rate: u32, channels: u16) -> Vec<u8> {
    let rate_idx = AAC_RATES
        .iter()
        .position(|r| *r == sample_rate)
        .unwrap_or(4) as u8;
    let chan_cfg = channels.clamp(1, 7) as u8;
    // AAC-LC 的 object type 是 2，ADTS 里 profile 字段存的是 type - 1
    let profile = 1u8;
    let len = (frame.len() + ADTS_HEADER) as u16;

    let mut out = Vec::with_capacity(len as usize);
    out.push(0xff);
    out.push(0xf1); // MPEG-4, layer 00, protection absent
    out.push((profile << 6) | (rate_idx << 2) | (chan_cfg >> 2));
    out.push(((chan_cfg & 0x03) << 6) | ((len >> 11) as u8 & 0x03));
    out.push(((len >> 3) & 0xff) as u8);
    out.push((((len as u8) & 0x07) << 5) | 0x1f); // buffer_fullness = 0x7ff
    out.push(0xfc); // buffer_fullness 低位 + 0 个 raw block
    out.extend_from_slice(frame);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adts_header_shape() {
        let frame = [0x21u8; 10];
        let out = adts_frame(&frame, 44100, 2);
        assert_eq!(out.len(), frame.len() + ADTS_HEADER);
        assert_eq!(out[0], 0xff, "ADTS 同步字");
        assert_eq!(out[1] & 0xf0, 0xf0, "MPEG-4 + 无 CRC");
        // 采样率索引 4（44100）应落在 byte2 的中 4 位
        assert_eq!((out[2] >> 2) & 0x0f, 4);
        // 帧长 17 = 7 + 10
        let len = ((u16::from(out[3] & 0x03) << 11)
            | (u16::from(out[4]) << 3)
            | (u16::from(out[5]) >> 5)) as usize;
        assert_eq!(len, frame.len() + ADTS_HEADER, "帧长字段");
        assert_eq!(&out[ADTS_HEADER..], &frame[..], "负载原样保留");
    }

    #[test]
    fn adts_known_rate_maps_to_correct_index() {
        for (rate, idx) in [(48000u32, 3u8), (44100, 4), (16000, 8)] {
            let out = adts_frame(&[0u8; 4], rate, 2);
            assert_eq!((out[2] >> 2) & 0x0f, idx, "rate = {rate}");
        }
    }

    #[test]
    fn adts_unknown_rate_falls_back_to_44100() {
        let out = adts_frame(&[0u8; 4], 12345, 2);
        assert_eq!((out[2] >> 2) & 0x0f, 4);
    }

    #[test]
    fn adts_mono_sets_channel_config() {
        let out = adts_frame(&[0u8; 4], 44100, 1);
        let cfg = ((out[2] & 0x01) << 2) | (out[3] >> 6);
        assert_eq!(cfg, 1);
    }

    #[test]
    fn asc_index_maps_to_44100_stereo() {
        // 布局：byte0 = [AOT:5][sfi:3]  byte1 = [sfi:1][chan:3]  byte2 = [chan:1]
        // sfi 4(44100) = 0b0100 → byte0 低 3 位 = 0b010，byte1 bit7 = 0
        // 声道 2 = 0b0010 → byte1 bit4 = 0x10，byte2 bit7 = 0
        let cfg = [0x02, 0x10, 0x00];
        let f = audio_format_from_asc(&cfg);
        assert_eq!(f.sample_rate, 44100);
        assert_eq!(f.channels, 2);
    }

    #[test]
    fn asc_index_spans_two_bytes() {
        // sfi 3(48000) = 0b0011 → byte0 低 3 位 = 0b001，byte1 bit7 = 1
        let cfg = [0x01, 0x80, 0x00];
        assert_eq!(audio_format_from_asc(&cfg).sample_rate, 48000);

        // sfi 8(16000) = 0b1000 → byte0 低 3 位 = 0b100，byte1 bit7 = 0
        let cfg = [0x04, 0x10, 0x00];
        assert_eq!(audio_format_from_asc(&cfg).sample_rate, 16000);

        // sfi 12(7350) = 0b1100 → byte0 低 3 位 = 0b110，byte1 bit7 = 0
        let cfg = [0x06, 0x10, 0x00];
        assert_eq!(audio_format_from_asc(&cfg).sample_rate, 7350);
    }

    #[test]
    fn asc_mono_channel_config() {
        // 声道 1 = 0b0001 → byte1 bit6..bit4 = 0，byte2 bit7 = 1
        let cfg = [0x02, 0x00, 0x80];
        assert_eq!(audio_format_from_asc(&cfg).channels, 1);
    }

    #[test]
    fn asc_stereo_channel_config() {
        // 声道 2 = 0b0010 → byte1 bit4 = 0x10，byte2 bit7 = 0
        let cfg = [0x02, 0x10, 0x00];
        assert_eq!(audio_format_from_asc(&cfg).channels, 2);
    }

    #[test]
    fn default_is_44100_stereo() {
        let d = AudioFormat::default();
        assert_eq!((d.sample_rate, d.channels), (44100, 2));
    }

    #[test]
    fn descriptor_len_single_byte() {
        let data = [0x03u8];
        assert_eq!(read_descriptor_len(&data, 0), Some((3, 1)));
    }

    #[test]
    fn descriptor_len_two_bytes() {
        let data = [0x81u8, 0x00u8];
        assert_eq!(read_descriptor_len(&data, 0), Some((128, 2)));
    }

    #[test]
    fn truncated_descriptor_returns_none() {
        assert_eq!(read_descriptor_len(&[0x81], 0), None);
    }

    #[test]
    fn all_continuation_bytes_returns_none() {
        // 描述符长度最多 4 字节，续位一直置 1 就是非法编码，不能无限读下去
        assert_eq!(
            read_descriptor_len(&[0x80, 0x80, 0x80, 0x80, 0x01], 0),
            None
        );
    }
}

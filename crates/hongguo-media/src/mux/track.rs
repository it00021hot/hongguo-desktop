//! 轨道素材构造：把编码输出与源音轨变成 [`crate::mp4::TrackSpec`] 需要的 box。
//!
//! 这一层只管「一个 box 长什么样」，不管时间轴怎么算、样本怎么交织——
//! 那是 [`crate::mux`] 的事。分开的理由：box 的字段布局是**规范**（ISO/IEC
//! 14496-12 / 14496-15），照着写死即可；而时间轴是**本仓的取舍**，会随
//! 编码器行为变化，两者的变更节奏完全不同。

use hongguo_video::r#box::build_box;

use super::avcc::{self, AvcConfig};
use super::{MEDIA_TIMESCALE, MOVIE_TIMESCALE};
use crate::mp4::TrackSpec;

/// 视频轨的全部素材。
pub struct VideoSpec {
    /// 轨道标识（1 起）
    pub track_id: u32,
    pub width: u32,
    pub height: u32,
    /// 参数集
    pub avc: AvcConfig,
    /// 逐样本大小
    pub sizes: Vec<u32>,
    /// `stts` 游程
    pub stts: Vec<(u32, u32)>,
    /// `ctts` 游程。空 = 没有这张表（无 B 帧）。
    pub ctts: Vec<(u32, i32)>,
    /// `stss` 同步样本号。`None` = 全部样本都是关键帧。
    pub stss: Option<Vec<u32>>,
    /// 媒体时长（media timescale）
    pub duration: u64,
}

impl VideoSpec {
    /// 组装成写出层要的轨道素材。
    pub fn build(&self) -> TrackSpec {
        TrackSpec {
            tkhd: tkhd(
                self.track_id,
                to_movie(self.duration),
                self.width,
                self.height,
                0,
            ),
            mdhd: mdhd(MEDIA_TIMESCALE, self.duration),
            hdlr: hdlr(b"vide", "VideoHandler"),
            stsd: video_stsd(self.width, self.height, &self.avc),
            header: vmhd(),
            dinf: dinf(),
            sizes: self.sizes.clone(),
            stts: self.stts.clone(),
            ctts: self.ctts.clone(),
            stss: self.stss.clone(),
            // 编码产物没有源 edit 可搬：presentation 映射就是媒体时间轴本身
            edits: Vec::new(),
        }
    }
}

/// 音频轨的全部素材。
pub struct AudioSpec {
    /// 轨道标识（1 起）
    pub track_id: u32,
    /// 声道数
    pub channels: u16,
    /// 采样率
    pub sample_rate: u32,
    /// 源音轨的 `esds` box 原始字节。`None` = 源里没有，按 AAC-LC 现造。
    pub esds: Option<Vec<u8>>,
    /// 逐样本大小
    pub sizes: Vec<u32>,
    /// `stts` 游程
    pub stts: Vec<(u32, u32)>,
    /// 媒体时长（media timescale）
    pub duration: u64,
}

impl AudioSpec {
    /// 组装成写出层要的轨道素材。
    pub fn build(&self) -> TrackSpec {
        TrackSpec {
            tkhd: tkhd(self.track_id, to_movie(self.duration), 0, 0, 0x0100),
            mdhd: mdhd(MEDIA_TIMESCALE, self.duration),
            hdlr: hdlr(b"soun", "SoundHandler"),
            stsd: audio_stsd(self.channels, self.sample_rate, self.esds.as_deref()),
            header: smhd(),
            dinf: dinf(),
            sizes: self.sizes.clone(),
            stts: self.stts.clone(),
            // 音频没有 B 帧，也就没有合成偏移
            ctts: Vec::new(),
            stss: None,
            edits: Vec::new(),
        }
    }
}

/// 媒体刻度 → 电影刻度（毫秒）。
pub fn to_movie(media_ticks: u64) -> u64 {
    media_ticks * u64::from(MOVIE_TIMESCALE) / u64::from(MEDIA_TIMESCALE)
}

/// `mvhd`：电影级头。
pub fn mvhd(duration_movie: u64, next_track_id: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend(timing_head(MOVIE_TIMESCALE, duration_movie));
    p.extend(0x0001_0000u32.to_be_bytes()); // rate = 1.0
    p.extend(0x0100u16.to_be_bytes()); // volume = 1.0
    p.extend(0u16.to_be_bytes()); // reserved
    p.extend(0u64.to_be_bytes()); // reserved
    p.extend(matrix());
    p.extend(
        [0u32; 6]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect::<Vec<u8>>(),
    );
    p.extend(next_track_id.to_be_bytes());
    build_box(b"mvhd", &p)
}

/// `tkhd`：轨道头。
///
/// `volume` 只有音频轨非零（0x0100 = 1.0）；`width` / `height` 是 16.16 定点，
/// 视频轨填分辨率，音频轨填 0。
pub fn tkhd(track_id: u32, duration_movie: u64, width: u32, height: u32, volume: u16) -> Vec<u8> {
    let mut p = Vec::new();
    if duration_movie > u64::from(u32::MAX) {
        p.push(1); // version 1
        p.extend([0u8; 3]); // flags(track_enabled | track_in_movie)
        p.extend(0u64.to_be_bytes()); // creation_time
        p.extend(0u64.to_be_bytes()); // modification_time
        p.extend(track_id.to_be_bytes());
        p.extend(0u32.to_be_bytes()); // reserved
        p.extend(duration_movie.to_be_bytes());
    } else {
        p.extend(0x0000_0003u32.to_be_bytes()); // version 0 + flags
        p.extend(0u32.to_be_bytes()); // creation_time
        p.extend(0u32.to_be_bytes()); // modification_time
        p.extend(track_id.to_be_bytes());
        p.extend(0u32.to_be_bytes()); // reserved
        p.extend((duration_movie as u32).to_be_bytes());
    }
    p.extend(0u64.to_be_bytes()); // reserved[2]
    p.extend(0u16.to_be_bytes()); // layer
    p.extend(0u16.to_be_bytes()); // alternate_group
    p.extend(volume.to_be_bytes());
    p.extend(0u16.to_be_bytes()); // reserved
    p.extend(matrix());
    p.extend((width << 16).to_be_bytes());
    p.extend((height << 16).to_be_bytes());
    build_box(b"tkhd", &p)
}

/// `mdhd`：媒体头（时基 + 时长）。
pub fn mdhd(timescale: u32, duration: u64) -> Vec<u8> {
    let mut p = timing_head(timescale, duration);
    p.extend(0u16.to_be_bytes()); // language = und
    p.extend(0u16.to_be_bytes()); // pre_defined
    build_box(b"mdhd", &p)
}

/// 「version + flags + creation + modification + timescale + duration」。
///
/// 时长装不下 32 位时用 version 1（64 位时长）——媒体时基下超过 13 小时才会
/// 触发，但写截断的时长会让播放器把片尾吃掉。
fn timing_head(timescale: u32, duration: u64) -> Vec<u8> {
    let mut p = Vec::new();
    if duration > u64::from(u32::MAX) {
        p.push(1); // version 1
        p.extend([0u8; 3]); // flags
        p.extend(0u64.to_be_bytes()); // creation_time
        p.extend(0u64.to_be_bytes()); // modification_time
        p.extend(timescale.to_be_bytes());
        p.extend(duration.to_be_bytes());
    } else {
        p.extend(0u32.to_be_bytes()); // version 0 + flags
        p.extend(0u32.to_be_bytes()); // creation_time
        p.extend(0u32.to_be_bytes()); // modification_time
        p.extend(timescale.to_be_bytes());
        p.extend((duration as u32).to_be_bytes());
    }
    p
}

/// `hdlr`：处理程序类型与名字。
pub fn hdlr(kind: &[u8; 4], name: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend(0u32.to_be_bytes()); // version 0 + flags
    p.extend(0u32.to_be_bytes()); // pre_defined
    p.extend_from_slice(kind);
    p.extend([0u8; 12]); // reserved
    p.extend_from_slice(name.as_bytes());
    p.push(0); // 名字是 C 字符串
    build_box(b"hdlr", &p)
}

/// `vmhd`：视频媒体头。
pub fn vmhd() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend(0x0000_0001u32.to_be_bytes()); // version 0 + flags = 1（规范要求置位）
    p.extend(0u16.to_be_bytes()); // graphicsmode
    p.extend([0u8; 6]); // opcolor
    build_box(b"vmhd", &p)
}

/// `smhd`：音频媒体头。
pub fn smhd() -> Vec<u8> {
    let mut p = Vec::new();
    p.extend(0u32.to_be_bytes()); // version 0 + flags
    p.extend(0u16.to_be_bytes()); // balance
    p.extend(0u16.to_be_bytes()); // reserved
    build_box(b"smhd", &p)
}

/// `dinf`：数据引用。
///
/// 只声明「数据就在本文件里」（`url ` 的 flags = 1），不引用外部 URL——
/// 产物要能脱离源文件独立播放。
pub fn dinf() -> Vec<u8> {
    let mut url = Vec::new();
    url.extend(1u32.to_be_bytes()); // version 0 + flags = 1（自包含）
    let mut dref = Vec::new();
    dref.extend(0u32.to_be_bytes()); // version 0 + flags
    dref.extend(1u32.to_be_bytes()); // entry_count
    dref.extend(build_box(b"url ", &url));
    build_box(b"dinf", &build_box(b"dref", &dref))
}

/// `stsd`（视频）：`avc1` sample entry + `avcC`。
pub fn video_stsd(width: u32, height: u32, avc: &AvcConfig) -> Vec<u8> {
    let mut entry = Vec::new();
    entry.extend([0u8; 6]); // reserved
    entry.extend(1u16.to_be_bytes()); // data_reference_index
    entry.extend(0u16.to_be_bytes()); // pre_defined
    entry.extend(0u16.to_be_bytes()); // reserved
    entry.extend([0u8; 12]); // pre_defined[3]
    entry.extend((width as u16).to_be_bytes());
    entry.extend((height as u16).to_be_bytes());
    entry.extend(0x0048_0000u32.to_be_bytes()); // horizresolution = 72dpi
    entry.extend(0x0048_0000u32.to_be_bytes()); // vertresolution = 72dpi
    entry.extend(0u32.to_be_bytes()); // reserved
    entry.extend(1u16.to_be_bytes()); // frame_count
    entry.extend([0u8; 32]); // compressorname
    entry.extend(0x0018u16.to_be_bytes()); // depth
    entry.extend(0xffffu16.to_be_bytes()); // pre_defined
    entry.extend(avcc::avcc_box(avc));
    stsd_of(&build_box(b"avc1", &entry))
}

/// `stsd`（音频）：`mp4a` sample entry + `esds`。
pub fn audio_stsd(channels: u16, sample_rate: u32, esds: Option<&[u8]>) -> Vec<u8> {
    let mut entry = Vec::new();
    entry.extend([0u8; 6]); // reserved
    entry.extend(1u16.to_be_bytes()); // data_reference_index
    entry.extend([0u8; 8]); // reserved[2]
    entry.extend(channels.to_be_bytes());
    entry.extend(16u16.to_be_bytes()); // sample size
    entry.extend(0u16.to_be_bytes()); // pre_defined
    entry.extend(0u16.to_be_bytes()); // reserved
    entry.extend((sample_rate << 16).to_be_bytes()); // samplerate = 16.16 定点
    match esds {
        Some(bytes) => entry.extend_from_slice(bytes),
        None => entry.extend(build_esds(sample_rate, channels)),
    }
    stsd_of(&build_box(b"mp4a", &entry))
}

/// `stsd`：样本描述表（一条轨一个 sample entry）。
fn stsd_of(entry: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend(0u32.to_be_bytes()); // version 0 + flags
    p.extend(1u32.to_be_bytes()); // entry_count
    p.extend_from_slice(entry);
    build_box(b"stsd", &p)
}

/// 现造一份 AAC-LC 的 `esds`。
///
/// 源音轨没有 `esds` 时才用：采样率与声道取自源的 sample entry，
/// `AudioSpecificConfig` 按 AAC-LC 编码（对象类型 2）。没有它，播放器
/// 拿不到解码器初始化参数，音轨解出来是噪声。
fn build_esds(sample_rate: u32, channels: u16) -> Vec<u8> {
    let asc = audio_specific_config(sample_rate, channels);

    let mut dsi = vec![0x05, asc.len() as u8];
    dsi.extend_from_slice(&asc);

    let mut dcd = vec![0x40, 0x15, 0, 0, 0]; // objectType / streamType / bufferSizeDB
    dcd.extend(0u32.to_be_bytes()); // maxBitrate
    dcd.extend(0u32.to_be_bytes()); // avgBitrate
    dcd.extend(dsi);

    let mut dec_config = vec![0x04, dcd.len() as u8];
    dec_config.extend(dcd);

    let mut es = vec![0x00, 0x01, 0x00]; // ES_ID + flags
    es.extend(dec_config);
    es.extend([0x06, 0x01, 0x02]); // SLConfigDescriptor

    let mut es_desc = vec![0x03, es.len() as u8];
    es_desc.extend(es);

    let mut p = vec![0u8; 4]; // version 0 + flags
    p.extend(es_desc);
    build_box(b"esds", &p)
}

/// AAC-LC 的 `AudioSpecificConfig`（2 字节）。
///
/// 位布局（ISO/IEC 14496-3）：`audioObjectType` 5 bit + `samplingFrequencyIndex`
/// 4 bit + `channelConfiguration` 4 bit，三个字段都跨字节，按整字节取会整体错位。
fn audio_specific_config(sample_rate: u32, channels: u16) -> [u8; 2] {
    const RATES: [u32; 13] = [
        96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
    ];
    let sfi = RATES.iter().position(|r| *r == sample_rate).unwrap_or(4) as u8;
    let chan = (channels.min(15) as u8) & 0x0f;
    [
        (2u8 << 3) | (sfi >> 1),        // audioObjectType = 2 + sfi 高 3 位
        ((sfi & 1) << 7) | (chan << 3), // sfi 低 1 位 + channelConfiguration
    ]
}

/// 单位矩阵（恒等变换）。
fn matrix() -> Vec<u8> {
    [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn avc() -> AvcConfig {
        AvcConfig {
            sps: vec![0x67, 0x64, 0x00, 0x1f],
            pps: vec![0x68, 0xeb],
        }
    }

    #[test]
    fn video_stsd_carries_the_resolution_and_the_avcc() {
        let stsd = video_stsd(1920, 1080, &avc());
        // stsd 载荷：version+flags(4) + entry_count(4) + entry box 头(8) + entry 载荷
        assert_eq!(&stsd[8..12], &[0, 0, 0, 0]);
        assert_eq!(&stsd[12..16], &[0, 0, 0, 1]);
        assert_eq!(&stsd[20..24], b"avc1");
        // sample entry 的固定体 78 字节，之后是 avcC
        assert_eq!(&stsd[24 + 24..24 + 26], &[0x07, 0x80], "1920 = 0x0780");
        assert_eq!(&stsd[24 + 26..24 + 28], &[0x04, 0x38], "1080 = 0x0438");
        // sample entry 的固定体 78 字节之后是 avcC box（头 8 字节）
        assert_eq!(&stsd[24 + 78 + 4..24 + 78 + 8], b"avcC");
    }

    #[test]
    fn audio_stsd_carries_the_source_esds_verbatim() {
        // 源音轨的 esds 是**整只 box**（含头），搬运时原样嵌进 sample entry
        let mut esds = Vec::new();
        esds.extend(28u32.to_be_bytes());
        esds.extend_from_slice(b"esds");
        esds.extend_from_slice(&[0xAA; 20]);
        let stsd = audio_stsd(2, 44100, Some(&esds));
        assert_eq!(&stsd[20..24], b"mp4a");
        // 固定体 28 字节之后是 esds box（头 8 字节）
        assert_eq!(&stsd[24 + 28 + 4..24 + 28 + 8], b"esds");
        assert_eq!(&stsd[24 + 28 + 8..24 + 28 + 8 + 20], &[0xAA; 20]);
    }

    #[test]
    fn audio_stsd_falls_back_to_a_built_esds() {
        let stsd = audio_stsd(2, 44100, None);
        assert_eq!(&stsd[24 + 28 + 4..24 + 28 + 8], b"esds");
    }

    #[test]
    fn the_built_asc_describes_aac_lc_44100_stereo() {
        // AOT=2、sfi=4（44100）、channelConfiguration=2
        assert_eq!(audio_specific_config(44100, 2), [0x12, 0x10]);
    }

    #[test]
    fn movie_ticks_convert_from_media_ticks() {
        assert_eq!(to_movie(90_000), 1000);
        assert_eq!(to_movie(45_000), 500);
    }
}

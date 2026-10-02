//! 最小 MP4 字节构造（仅测试用）。
//!
//! 只搭 [`super::sample_table::collect_tracks`] 真会读到的那几层：
//! `ftyp` + `moov/trak/tkhd/hdlr/stbl/stsd`。样本表与 `mdat` 跟「编码参数
//! 是否一致」无关，不搭——搭了只会让断言的对象变模糊。

use super::r#box::build_box;

/// 一条轨道的编码参数。
#[derive(Debug, Clone)]
pub struct TrackSpec {
    /// 轨道标识
    pub track_id: u32,
    /// 是否为视频轨（决定 `hdlr` 写 `vide` 还是 `soun`）
    pub is_video: bool,
    /// 编码四字符码
    pub codec: &'static [u8; 4],
    /// 视频编码分辨率
    pub width: u32,
    /// 视频编码分辨率
    pub height: u32,
    /// 音频声道数
    pub channels: u16,
    /// 音频采样率（Hz，取 16.16 定点的整数部分，故取值范围是 u16）
    pub sample_rate: u32,
}

impl TrackSpec {
    /// 视频轨。音频字段留 0。
    pub fn video(track_id: u32, codec: &'static [u8; 4], width: u32, height: u32) -> Self {
        Self {
            track_id,
            is_video: true,
            codec,
            width,
            height,
            channels: 0,
            sample_rate: 0,
        }
    }

    /// 音频轨。视频字段留 0。
    pub fn audio(track_id: u32, codec: &'static [u8; 4], channels: u16, sample_rate: u32) -> Self {
        Self {
            track_id,
            is_video: false,
            codec,
            width: 0,
            height: 0,
            channels,
            sample_rate,
        }
    }
}

/// 构造 FullBox：`version(1) + flags(3)` 由本函数写入（全 0），
/// `payload` 只放紧随其后的字段（如 `entry_count` / `track_ID`）。
pub fn full_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut body = vec![0u8, 0, 0, 0];
    body.extend_from_slice(payload);
    build_box(kind, &body)
}

/// 造一个只含编码信息的最小 MP4，轨道按给定顺序排列。
pub fn mp4(tracks: &[TrackSpec]) -> Vec<u8> {
    let ftyp = build_box(b"ftyp", b"isom\x00\x00\x02\x00isomiso2");
    let mut moov_payload = Vec::new();
    for spec in tracks {
        moov_payload.extend_from_slice(&trak(spec));
    }
    let moov = build_box(b"moov", &moov_payload);
    [ftyp, moov].concat()
}

fn trak(spec: &TrackSpec) -> Vec<u8> {
    // tkhd：creation(4) + modification(4) 之后才是 track_ID
    let mut tkhd_payload = vec![0u8; 8];
    tkhd_payload.extend_from_slice(&spec.track_id.to_be_bytes());
    let tkhd = full_box(b"tkhd", &tkhd_payload);

    // hdlr：pre_defined(4) 之后是 handler_type
    let mut hdlr_payload = vec![0u8; 4];
    hdlr_payload.extend_from_slice(if spec.is_video { b"vide" } else { b"soun" });
    let hdlr = full_box(b"hdlr", &hdlr_payload);

    let entry = if spec.is_video {
        visual_sample_entry(spec)
    } else {
        audio_sample_entry(spec)
    };
    let mut stsd_payload = 1u32.to_be_bytes().to_vec(); // entry_count
    stsd_payload.extend_from_slice(&entry);
    let stsd = full_box(b"stsd", &stsd_payload);

    // stbl 直接挂在 mdia 下：find_box 的容器白名单里有 mdia / stbl，
    // 中间的 minf 对 collect_tracks 没有影响
    let mdia = build_box(
        b"mdia",
        &[&hdlr[..], &build_box(b"stbl", &stsd)[..]].concat(),
    );
    build_box(b"trak", &[&tkhd[..], &mdia[..]].concat())
}

/// VisualSampleEntry：载荷前 24 字节是 reserved / data_reference_index /
/// pre_defined，之后才是 width(24) 与 height(26)。
fn visual_sample_entry(spec: &TrackSpec) -> Vec<u8> {
    let mut payload = vec![0u8; 24];
    payload.extend_from_slice(&(spec.width as u16).to_be_bytes());
    payload.extend_from_slice(&(spec.height as u16).to_be_bytes());
    // horiz/vert resolution、frame_count、compressorname、depth、pre_defined：
    // 规范里全为 0，占位即可，解析层不读它们
    payload.extend_from_slice(&[0u8; 50]);
    build_box(spec.codec, &payload)
}

/// AudioSampleEntry：载荷前 16 字节是 reserved，之后是 channelcount(16) /
/// samplesize(18) / pre_defined(20) / reserved(22)，再往后是 samplerate(24)。
fn audio_sample_entry(spec: &TrackSpec) -> Vec<u8> {
    let mut payload = vec![0u8; 16];
    payload.extend_from_slice(&spec.channels.to_be_bytes());
    payload.extend_from_slice(&16u16.to_be_bytes()); // samplesize
    payload.extend_from_slice(&[0u8; 4]); // pre_defined + reserved
                                          // samplerate 是 16.16 定点，小数部分留 0
    payload.extend_from_slice(&(spec.sample_rate << 16).to_be_bytes());
    payload.extend_from_slice(&[0u8; 4]); // 其余字段
    build_box(spec.codec, &payload)
}

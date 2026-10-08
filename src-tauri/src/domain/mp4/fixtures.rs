//! 最小 MP4 字节构造（仅测试用）。
//!
//! 两档：
//!
//! - [`mp4`]：只搭 [`super::sample_table::collect_tracks`] 真会读到的那几层，
//!   样本表与 `mdat` 跟「编码参数是否一致」无关，不搭——搭了只会让断言的对象变模糊。
//! - [`mp4_with_samples`]：带真实样本表与 `mdat`，给拼接索引的测试用。

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

/// 一条轨道在 fixture 里的样本与时间轴。
#[derive(Debug, Clone)]
pub struct TrackPlan {
    pub is_video: bool,
    /// 每个样本的字节
    pub samples: Vec<Vec<u8>>,
    /// 每个样本的 `stts` 增量
    pub durations: Vec<u32>,
    /// 每样本的合成偏移（`ctts`）。`Some` 才写这张表。
    pub composition: Option<Vec<i32>>,
    /// 每隔几个样本放一个关键帧。`None` = 不写 `stss` = 全部样本都是关键帧。
    pub sync_every: Option<u32>,
}

impl TrackPlan {
    /// 视频轨：每样本 1 个单位时长，每 5 帧一个关键帧，没有 `ctts`。
    pub fn video(samples: Vec<Vec<u8>>) -> Self {
        Self {
            is_video: true,
            durations: vec![1; samples.len()],
            composition: None,
            sync_every: Some(5),
            samples,
        }
    }

    /// 音频轨：每样本 1 个单位时长，不写 `stss`（音频没有关键帧概念）。
    pub fn audio(samples: Vec<Vec<u8>>) -> Self {
        Self {
            is_video: false,
            durations: vec![1; samples.len()],
            composition: None,
            sync_every: None,
            samples,
        }
    }

    /// 带 B 帧的视频：合成偏移在前半程为 0、后半程为负，模拟真实的重排序。
    pub fn video_with_b_frames(samples: Vec<Vec<u8>>) -> Self {
        let n = samples.len();
        let composition = (0..n).map(|i| if i < n / 2 { 0 } else { -1024 }).collect();
        Self {
            is_video: true,
            durations: vec![1; n],
            composition: Some(composition),
            sync_every: Some(5),
            samples,
        }
    }

    /// 每样本时长各不相同，用来验证 `stts` 游程拼接。
    pub fn video_ragged(samples: Vec<Vec<u8>>) -> Self {
        let durations = (0..samples.len()).map(|i| 1 + (i as u32 % 4)).collect();
        Self {
            is_video: true,
            durations,
            composition: None,
            sync_every: Some(3),
            samples,
        }
    }
}

/// 造一个带真实样本表与 `mdat` 的 MP4。
///
/// 布局是 `[ftyp][mdat][moov]`（moov 在后）：`stco` 的偏移不依赖 `moov` 自己的
/// 大小，构造时就不必「先量一次再重算」。被测的拼接代码走的是另一条路
/// （faststart + 两次构建），两者不共享逻辑，才不会一起错。
///
/// 多轨时按 `轨0样本0, 轨1样本0, 轨0样本1, …` 轮转交织，和真实短剧一致。
pub fn mp4_with_samples(tracks: &[TrackPlan]) -> Vec<u8> {
    let ftyp = build_box(b"ftyp", b"isom\x00\x00\x02\x00isomiso2");
    let mdat_header_len = 8u64;
    let data_start = ftyp.len() as u64 + mdat_header_len;

    // 轮转交织：每次从每条轨取下一个样本
    let mut payload: Vec<u8> = Vec::new();
    let mut offsets: Vec<Vec<u64>> = vec![Vec::new(); tracks.len()];
    let mut cursor = data_start;
    let mut next = vec![0usize; tracks.len()];
    loop {
        let mut progressed = false;
        for (t, plan) in tracks.iter().enumerate() {
            if next[t] < plan.samples.len() {
                progressed = true;
                offsets[t].push(cursor);
                payload.extend_from_slice(&plan.samples[next[t]]);
                cursor += plan.samples[next[t]].len() as u64;
                next[t] += 1;
            }
        }
        if !progressed {
            break;
        }
    }

    let mdat = build_box(b"mdat", &payload);

    let mut moov = full_box(b"mvhd", &{
        let mut p = vec![0u8; 8]; // creation + modification
        p.extend(1000u32.to_be_bytes()); // timescale
        p.extend(
            tracks
                .iter()
                .map(|t| t.durations.iter().sum::<u32>())
                .sum::<u32>()
                .to_be_bytes(),
        ); // duration = 全轨时长之和
        p.extend(0x0001_0000u32.to_be_bytes()); // rate
        p.extend(0x0100u16.to_be_bytes()); // volume
        p.extend(0u16.to_be_bytes()); // reserved
        p.extend([0u8; 8]); // reserved
        p.extend(matrix()); // unity matrix
        p.extend([0u8; 24]); // pre_defined
        p.extend((tracks.len() as u32 + 1).to_be_bytes()); // next_track_ID
        p
    });

    for (i, plan) in tracks.iter().enumerate() {
        moov.extend(build_trak(i as u32 + 1, plan, &offsets[i]));
    }
    let moov = build_box(b"moov", &moov);

    [ftyp, mdat, moov].concat()
}

/// 单位矩阵
fn matrix() -> Vec<u8> {
    let mut m = vec![0u8; 36];
    m[0] = 0x00;
    m[1] = 0x01;
    m[10] = 0x00;
    m[11] = 0x01;
    m[20] = 0x40;
    m[22] = 0x40;
    m
}

fn build_trak(track_id: u32, plan: &TrackPlan, offsets: &[u64]) -> Vec<u8> {
    let codec: &[u8; 4] = if plan.is_video { b"hvc1" } else { b"mp4a" };
    let spec = if plan.is_video {
        TrackSpec::video(track_id, codec, 1920, 1080)
    } else {
        TrackSpec::audio(track_id, codec, 2, 44_100)
    };

    // 真实封装器会把时长写进 tkhd / mdhd：拼接时正是靠累加各集的这两个字段
    // 得出全集时长，写成 0 的话那些断言就全在验 0 + 0 + 0。
    let duration: u32 = plan.durations.iter().sum();

    // tkhd：creation(4) modification(4) track_ID(4) reserved(4) duration(4)
    let mut tkhd_payload = vec![0u8; 8];
    tkhd_payload.extend(track_id.to_be_bytes());
    tkhd_payload.extend(0u32.to_be_bytes());
    tkhd_payload.extend(duration.to_be_bytes());
    tkhd_payload.extend([0u8; 8]); // reserved
    tkhd_payload.extend(0u16.to_be_bytes()); // layer
    tkhd_payload.extend(0u16.to_be_bytes()); // alternate_group
    tkhd_payload.extend(0u16.to_be_bytes()); // volume
    tkhd_payload.extend(0u16.to_be_bytes()); // reserved
    tkhd_payload.extend(matrix());
    tkhd_payload.extend(spec.width.to_be_bytes());
    tkhd_payload.extend(spec.height.to_be_bytes());
    let tkhd = full_box(b"tkhd", &tkhd_payload);

    // mdhd：creation(4) modification(4) timescale(4) duration(4)
    let mut mdhd_payload = vec![0u8; 8];
    mdhd_payload.extend(1000u32.to_be_bytes()); // timescale
    mdhd_payload.extend(duration.to_be_bytes());
    mdhd_payload.extend(0u16.to_be_bytes()); // language
    mdhd_payload.extend(0u16.to_be_bytes()); // pre_defined
    let mdhd = full_box(b"mdhd", &mdhd_payload);

    // hdlr：pre_defined(4) handler_type(4)
    let mut hdlr_payload = vec![0u8; 4];
    hdlr_payload.extend(if plan.is_video { b"vide" } else { b"soun" });
    hdlr_payload.extend([0u8; 12]);
    let hdlr = full_box(b"hdlr", &hdlr_payload);

    let media_header = if plan.is_video {
        full_box(b"vmhd", &[0, 1, 0, 0, 0, 0, 0, 0])
    } else {
        full_box(b"smhd", &[0, 0, 0, 0])
    };

    let dinf = build_box(b"dinf", &full_box(b"dref", &[0, 0, 0, 1, 0, 0, 0, 12]));

    let mut stbl = build_box(b"stsd", &{
        let mut p = 1u32.to_be_bytes().to_vec();
        p.extend(if plan.is_video {
            visual_sample_entry(&spec)
        } else {
            audio_sample_entry(&spec)
        });
        p
    });
    stbl.extend(stts_box(&plan.durations));
    if let Some(comp) = &plan.composition {
        stbl.extend(ctts_box(comp));
    }
    if let Some(every) = plan.sync_every
        && every > 0 {
            stbl.extend(stss_box(plan.samples.len(), every));
        }
    stbl.extend(stsc_one_per_chunk());
    stbl.extend(stsz_box(plan));
    stbl.extend(stco_box(offsets));
    let stbl = build_box(b"stbl", &stbl);

    let minf = build_box(b"minf", &[&media_header[..], &dinf[..], &stbl[..]].concat());
    let mdia = build_box(b"mdia", &[&mdhd[..], &hdlr[..], &minf[..]].concat());
    build_box(b"trak", &[&tkhd[..], &mdia[..]].concat())
}

fn stts_box(durations: &[u32]) -> Vec<u8> {
    // 相邻同值合并成游程，和真实封装器一致
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for d in durations {
        match runs.last_mut() {
            Some(last) if last.1 == *d => last.0 += 1,
            _ => runs.push((1, *d)),
        }
    }
    let mut p = 0u32.to_be_bytes().to_vec();
    p.extend((runs.len() as u32).to_be_bytes());
    for (n, d) in runs {
        p.extend(n.to_be_bytes());
        p.extend(d.to_be_bytes());
    }
    build_box(b"stts", &p)
}

fn ctts_box(composition: &[i32]) -> Vec<u8> {
    let mut runs: Vec<(u32, i32)> = Vec::new();
    for o in composition {
        match runs.last_mut() {
            Some(last) if last.1 == *o => last.0 += 1,
            _ => runs.push((1, *o)),
        }
    }
    let mut p = vec![1u8, 0, 0, 0]; // version 1 = 有符号，flags 占后 3 字节
    p.extend((runs.len() as u32).to_be_bytes());
    for (n, o) in runs {
        p.extend(n.to_be_bytes());
        p.extend((o as u32).to_be_bytes());
    }
    build_box(b"ctts", &p)
}

fn stss_box(sample_count: usize, every: u32) -> Vec<u8> {
    let nums: Vec<u32> = (1..=sample_count as u32)
        .filter(|n| (n - 1) % every == 0)
        .collect();
    let mut p = 0u32.to_be_bytes().to_vec();
    p.extend((nums.len() as u32).to_be_bytes());
    for n in nums {
        p.extend(n.to_be_bytes());
    }
    build_box(b"stss", &p)
}

fn stsc_one_per_chunk() -> Vec<u8> {
    let mut p = 0u32.to_be_bytes().to_vec();
    p.extend(1u32.to_be_bytes());
    p.extend(1u32.to_be_bytes());
    p.extend(1u32.to_be_bytes());
    p.extend(1u32.to_be_bytes());
    build_box(b"stsc", &p)
}

fn stsz_box(plan: &TrackPlan) -> Vec<u8> {
    let mut p = 0u32.to_be_bytes().to_vec(); // version + flags
    p.extend(0u32.to_be_bytes()); // sample_size = 0：逐样本给
    p.extend((plan.samples.len() as u32).to_be_bytes());
    for s in &plan.samples {
        p.extend((s.len() as u32).to_be_bytes());
    }
    build_box(b"stsz", &p)
}

fn stco_box(offsets: &[u64]) -> Vec<u8> {
    let mut p = 0u32.to_be_bytes().to_vec();
    p.extend((offsets.len() as u32).to_be_bytes());
    for o in offsets {
        p.extend((*o as u32).to_be_bytes());
    }
    build_box(b"stco", &p)
}

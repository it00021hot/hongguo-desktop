//! 流复制拼接（快速合并）。
//!
//! 不解码不编码，但**必须重写索引**：把 N 个各自从 t=0 开始的样本表拼成一条从 0
//! 连续到全集末尾的时间轴。整文件字节级顺序拼接做不到这件事——第 1 集的 `stco`
//! 只覆盖第 1 集的 `mdat`，播放器读完就停，后面的集拿不到索引。索引怎么重建见
//! [`index`]。
//!
//! 产物布局固定为 `[ftyp][moov][mdat]`（faststart）：moov 在前，播放器读文件头
//! 就能拿到时长与样本表，不必先跳过整个媒体数据。

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

mod index;

/// 按集号数字排序的待合并文件。
///
/// 必须按数字排：字符串排序会把第 10 集排到第 2 集前面。
pub fn sort_by_index(files: &[(u32, PathBuf)]) -> Vec<(u32, PathBuf)> {
    let mut v = files.to_vec();
    v.sort_by_key(|(i, _)| *i);
    v
}

/// 流复制合并。
///
/// 快速合并要求所有输入的编码一致：由 [`crate::media::codec_probe::check`] 探测、
/// 在 [`crate::service::merge_service::quick::quick_merge`] 上强制执行。
/// [`index::plan`] 里的结构检查是第二道闸，管的是探测不到的轨数、轨道顺序与
/// 电影时基——任一对不上都只会产出连索引都过不去的文件。
pub fn concat_copy(inputs: &[PathBuf], output: &Path) -> AppResult<(u64, usize)> {
    if inputs.is_empty() {
        return Err(AppError::Media("没有待合并的文件".into()));
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }

    let temp = crate::service::download_service::worker::temp_path_for(output);
    // 上一轮崩在中途会留下半个临时文件，先清掉再动手
    let _ = std::fs::remove_file(&temp);

    let prepared = prepare(inputs)?;
    let (size, count) = write_output(inputs, &temp, &prepared)?;

    std::fs::rename(&temp, output).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        AppError::Io(format!("原子替换失败: {e}"))
    })?;
    Ok((size, count))
}

/// 读出全部输入、规划索引、算出每个样本在产物里的偏移。
///
/// 解析只做一次：`write_output` 复用这里的结果，不重新读文件头——否则
/// 「规划时看到的内容」与「实际拷贝的内容」可能不是同一份。
struct Prepared {
    ftyp: Vec<u8>,
    moov: Vec<u8>,
    /// 全部样本字节数之和，也就是 `mdat` 的载荷长度
    total_payload: u64,
    /// 每集解析出的轨道与交织顺序
    sources: Vec<index::SourceFile>,
    /// 偏移是否必须用 64 位
    wide: bool,
}

fn prepare(inputs: &[PathBuf]) -> AppResult<Prepared> {
    let mut sources = Vec::with_capacity(inputs.len());
    let mut ftyp = Vec::new();
    for (i, path) in inputs.iter().enumerate() {
        let (f, moov) = read_header_boxes(path)?;
        if i == 0 {
            ftyp = f;
        }
        sources.push(index::read_source_file(&moov)?);
    }

    let plan = index::plan(&sources)?;

    let total_payload: u64 = sources
        .iter()
        .flat_map(|s| s.tracks.iter())
        .map(|t| t.info.samples.iter().map(|(_, size)| *size).sum::<u64>())
        .sum();
    let wide = index::needs_wide_offsets(total_payload);

    // moov 长度只取决于样本数与偏移宽度，与偏移的具体数值无关：
    // 先用占位偏移量一次长度，再带真实偏移重算一次。
    let placeholders: Vec<Vec<u64>> = plan
        .tracks
        .iter()
        .map(|t| vec![0u64; t.sizes.len()])
        .collect();
    let moov_len = index::build_moov(&plan, &placeholders, wide).len();
    let data_start = index::mdat_data_start(ftyp.len(), moov_len, wide);

    let mut offsets: Vec<Vec<u64>> = plan
        .tracks
        .iter()
        .map(|t| vec![0u64; t.sizes.len()])
        .collect();
    // 各轨已经并入的样本数：样本 s 在轨道 t 里的合并下标 = per_track[t] + s
    let mut per_track = vec![0usize; plan.tracks.len()];
    let mut cursor = data_start;
    for source in &sources {
        // order 按源偏移升序，所以这是一次顺序扫描
        for (t, s) in &source.order {
            let size = source.tracks[*t].info.samples[*s as usize].1;
            offsets[*t][per_track[*t] + *s as usize] = cursor;
            cursor += size;
        }
        for (t, count) in source.tracks.iter().enumerate() {
            per_track[t] += count.info.samples.len();
        }
    }

    let moov = index::build_moov(&plan, &offsets, wide);
    debug_assert_eq!(moov.len(), moov_len, "两次构建的 moov 长度应一致");

    Ok(Prepared {
        ftyp,
        moov,
        total_payload,
        sources,
        wide,
    })
}

fn write_output(inputs: &[PathBuf], temp: &Path, prepared: &Prepared) -> AppResult<(u64, usize)> {
    let mut out = std::fs::File::create(temp).map_err(|e| AppError::Io(e.to_string()))?;
    let write = |out: &mut std::fs::File, buf: &[u8]| -> AppResult<()> {
        out.write_all(buf).map_err(|e| AppError::Io(e.to_string()))
    };

    write(&mut out, &prepared.ftyp)?;
    write(&mut out, &prepared.moov)?;
    write(
        &mut out,
        &mdat_header(prepared.total_payload, prepared.wide),
    )?;

    let mut written = 0u64;
    let mut buf: Vec<u8> = Vec::new();
    for (path, source) in inputs.iter().zip(&prepared.sources) {
        let mut file = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
        let mut cursor = 0u64;
        for (t, s) in &source.order {
            let (at, len) = source.tracks[*t].info.samples[*s as usize];
            buf.resize(len as usize, 0);
            read_exact_at(&mut file, &mut cursor, at, &mut buf)?;
            write(&mut out, &buf)?;
            written += len;
        }
    }

    out.flush().map_err(|e| AppError::Io(e.to_string()))?;
    Ok((written, inputs.len()))
}

/// `mdat` 的 box 头。载荷装不下 32 位时用 largesize。
///
/// 返回的**长度就是真正写出去的字节数**——固定长度的数组会在窄偏移时多写出
/// 8 个零字节，所有样本偏移随之偏小。
fn mdat_header(total_payload: u64, wide: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    if wide {
        out.extend(1u32.to_be_bytes());
        out.extend(b"mdat");
        out.extend(total_payload.to_be_bytes());
    } else {
        let total = (index::mdat_header_len(false) as u64 + total_payload) as u32;
        out.extend(total.to_be_bytes());
        out.extend(b"mdat");
    }
    out
}

/// 从 `file` 的 `at` 处读满 `buf`。偏移单调递增时不会真的 seek。
fn read_exact_at(
    file: &mut std::fs::File,
    cursor: &mut u64,
    at: u64,
    buf: &mut [u8],
) -> AppResult<()> {
    if *cursor != at {
        file.seek(SeekFrom::Start(at))
            .map_err(|e| AppError::Io(e.to_string()))?;
        *cursor = at;
    }
    file.read_exact(buf)
        .map_err(|e| AppError::Io(e.to_string()))?;
    *cursor = at + buf.len() as u64;
    Ok(())
}

/// 读出顶层 `ftyp` 与 `moov` 的完整字节。
///
/// 按 box 声明的长度精确定位，不走「先读前 N 兆」那条路：moov 可能超过那个量，
/// 截断会让 `parse_boxes` **静默**少解析，产物就会「少几条样本表」而不报错。
fn read_header_boxes(path: &Path) -> AppResult<(Vec<u8>, Vec<u8>)> {
    let mut file = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    let file_len = file
        .metadata()
        .map_err(|e| AppError::Io(e.to_string()))?
        .len();

    let mut ftyp = None;
    let mut moov = None;
    let mut pos = 0u64;

    while pos + 8 <= file_len {
        file.seek(SeekFrom::Start(pos))
            .map_err(|e| AppError::Io(e.to_string()))?;
        let mut head = [0u8; 8];
        if file.read_exact(&mut head).is_err() {
            break;
        }
        let size32 = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as u64;
        let kind = [head[4], head[5], head[6], head[7]];

        let total = match size32 {
            // size == 1：真实长度是紧跟其后的 8 字节
            1 => {
                let mut large = [0u8; 8];
                file.read_exact(&mut large)
                    .map_err(|e| AppError::Io(e.to_string()))?;
                u64::from_be_bytes(large)
            }
            // size == 0：延伸到文件末尾
            0 => file_len - pos,
            n => n,
        };
        if total < 8 || pos + total > file_len {
            break;
        }

        let slot = if &kind == b"ftyp" {
            &mut ftyp
        } else if &kind == b"moov" {
            &mut moov
        } else {
            pos += total;
            continue;
        };
        if slot.is_none() {
            file.seek(SeekFrom::Start(pos))
                .map_err(|e| AppError::Io(e.to_string()))?;
            let mut buf = vec![0u8; total as usize];
            file.read_exact(&mut buf)
                .map_err(|e| AppError::Io(e.to_string()))?;
            *slot = Some(buf);
        }
        pos += total;
    }

    match (ftyp, moov) {
        (Some(ftyp), Some(moov)) => Ok((ftyp, moov)),
        _ => Err(AppError::Media(format!(
            "{} 缺 ftyp 或 moov，不是可拼接的 MP4",
            path.display()
        ))),
    }
}

#[cfg(test)]
#[path = "remux/e2e_tests.rs"]
mod e2e_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::mp4::fixtures::{TrackPlan, mp4_with_samples};
    use crate::domain::mp4::sample_table::collect_tracks;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hg-remux-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn episode(dir: &Path, vid_index: u32, tracks: Vec<TrackPlan>) -> PathBuf {
        let path = dir.join(format!("{vid_index}.mp4"));
        std::fs::write(&path, mp4_with_samples(&tracks)).unwrap();
        path
    }

    /// 单轨样本：大小随序号递增，字节是序号填充，回读时能逐个定位。
    fn video_samples(n: usize) -> Vec<Vec<u8>> {
        (0..n).map(|i| vec![(i % 251) as u8; 100 + i * 3]).collect()
    }

    #[test]
    fn sort_is_numeric_not_lexicographic() {
        let files = vec![
            (10u32, PathBuf::from("a")),
            (2, PathBuf::from("b")),
            (1, PathBuf::from("c")),
        ];
        let sorted = sort_by_index(&files);
        assert_eq!(
            sorted.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            vec![1, 2, 10]
        );
    }

    #[test]
    fn empty_input_errors() {
        assert!(concat_copy(&[], Path::new("/tmp/out.mp4")).is_err());
    }

    #[test]
    fn three_files_concatenate_into_one_continuous_timeline() {
        let dir = temp_dir("three");
        let inputs = vec![
            episode(&dir, 1, vec![TrackPlan::video(video_samples(3))]),
            episode(&dir, 2, vec![TrackPlan::video(video_samples(5))]),
            episode(&dir, 3, vec![TrackPlan::video(video_samples(2))]),
        ];
        let out = dir.join("合集.mp4");

        let (size, count) = concat_copy(&inputs, &out).unwrap();
        assert_eq!(count, 3);

        let data = std::fs::read(&out).unwrap();
        let tracks = collect_tracks(&data).unwrap();
        // 样本数是三集之和——字节级拼接做不到这一点
        assert_eq!(tracks[0].sample_count, 10);
        // 时长（每样本 1 个单位）同样是三集之和
        assert_eq!(tracks[0].media_duration, 10);
        let payload: u64 = (0..3).map(|i| 100 + i * 3).sum::<u64>()
            + (0..5).map(|i| 100 + i * 3).sum::<u64>()
            + (0..2).map(|i| 100 + i * 3).sum::<u64>();
        assert_eq!(size, payload, "mdat 载荷应是全部样本字节");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_sample_survives_in_the_right_order() {
        let dir = temp_dir("bytes");
        let a = video_samples(3);
        let b = video_samples(2);
        let inputs = vec![
            episode(&dir, 1, vec![TrackPlan::video(a.clone())]),
            episode(&dir, 2, vec![TrackPlan::video(b.clone())]),
        ];
        let out = dir.join("合集.mp4");
        concat_copy(&inputs, &out).unwrap();

        let data = std::fs::read(&out).unwrap();
        let tracks = collect_tracks(&data).unwrap();
        for (i, expected) in a.iter().chain(b.iter()).enumerate() {
            let (at, len) = tracks[0].samples[i];
            assert_eq!(len, expected.len() as u64, "第 {i} 个样本大小不对");
            assert_eq!(
                &data[at as usize..(at + len) as usize],
                expected.as_slice(),
                "第 {i} 个样本字节不对"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn composition_offsets_are_copied_verbatim_not_shifted() {
        // B 帧的合成偏移在拼接后必须原样保留：解码时间整体平移了各集时长，
        // 合成时间同步平移，两者差值（也就是 offset）不变。再加一次起点偏移，
        // 错位会逐集累积。
        let dir = temp_dir("ctts");
        let a = (0..4).map(|i| vec![(i + 1) as u8; 80 + i]).collect();
        let b = (0..4).map(|i| vec![(i + 9) as u8; 90 + i]).collect();
        let inputs = vec![
            episode(&dir, 1, vec![TrackPlan::video_with_b_frames(a)]),
            episode(&dir, 2, vec![TrackPlan::video_with_b_frames(b)]),
        ];
        let out = dir.join("合集.mp4");
        concat_copy(&inputs, &out).unwrap();

        let data = std::fs::read(&out).unwrap();
        let tracks = collect_tracks(&data).unwrap();
        assert_eq!(tracks[0].sample_count, 8);
        // 源里是 [0,0,-1024,-1024] 拼 [0,0,-1024,-1024]，合并后逐样本仍是同一组取值，
        // 没有任何一项被加上第 2 集的起点
        let offsets: Vec<i32> = tracks[0]
            .ctts
            .iter()
            .flat_map(|&(n, o)| std::iter::repeat_n(o, n as usize))
            .collect();
        assert_eq!(offsets, vec![0, 0, -1024, -1024, 0, 0, -1024, -1024]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_tracks_stay_interleaved_in_the_output() {
        let dir = temp_dir("interleave");
        let video = (0..4).map(|i| vec![0xAAu8; 60 + i]).collect();
        let audio = (0..4).map(|i| vec![0xBBu8; 20 + i]).collect();
        let inputs = vec![episode(
            &dir,
            1,
            vec![TrackPlan::video(video), TrackPlan::audio(audio)],
        )];
        let out = dir.join("合集.mp4");
        concat_copy(&inputs, &out).unwrap();

        let data = std::fs::read(&out).unwrap();
        let tracks = collect_tracks(&data).unwrap();
        assert_eq!(tracks.len(), 2);

        // 产物的 mdat 必须是「视频0 音频0 视频1 音频1 …」的交织排布，
        // 而不是把整条视频轨写完再写音频轨
        let v = &tracks[0].samples;
        let a = &tracks[1].samples;
        for i in 0..4 {
            assert!(a[i].0 > v[i].0, "第 {i} 轮里音频应排在视频之后");
            assert!(v[i].0 < a[i].0, "第 {i} 轮里视频应排在音频之前");
            if i + 1 < 4 {
                assert!(v[i + 1].0 > a[i].0, "第 {i} 轮不能被第 {} 轮盖住", i + 1);
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mismatched_track_counts_are_refused() {
        let dir = temp_dir("mismatch");
        let inputs = vec![
            episode(&dir, 1, vec![TrackPlan::video(video_samples(2))]),
            episode(
                &dir,
                2,
                vec![
                    TrackPlan::video(video_samples(2)),
                    TrackPlan::audio(video_samples(2)),
                ],
            ),
        ];
        let out = dir.join("合集.mp4");

        let err = concat_copy(&inputs, &out).expect_err("轨数不同必须拒绝");
        let text = err.to_string();
        assert!(text.contains("轨道"), "要说清是轨道结构问题: {text}");
        assert!(text.contains('2'), "要指到出问题的集号: {text}");
        assert!(!out.exists(), "拒绝时不产出文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_input_errors_and_cleans_temp() {
        let dir = temp_dir("bad");
        let out = dir.join("out.mp4");
        let missing = dir.join("missing.mp4");

        assert!(concat_copy(&[missing], &out).is_err());
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "失败后不应残留临时文件: {leftovers:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

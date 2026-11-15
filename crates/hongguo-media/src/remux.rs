//! 流复制拼接（快速合并）：不解码不编码，但**必须重写索引**——把 N 个各自从 t=0 开始的样本表拼成一条从 0 连续到全集末尾的时间轴。
//! 整文件字节级顺序拼接做不到这件事：第 1 集的 `stco` 只覆盖第 1 集的 `mdat`，播放器读完就停；索引怎么重建见 `index` 模块。
//! 产物布局固定为 `[ftyp][moov][mdat]`（faststart）。

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use hongguo_common::{AppError, AppResult};

use crate::mp4::{build_moov, mdat_data_start, mdat_header, needs_wide_offsets};

mod index;

/// 按集号数字排序的待合并文件。
///
/// 必须按数字排：字符串排序会把第 10 集排到第 2 集前面。
pub fn sort_by_index(files: &[(u32, PathBuf)]) -> Vec<(u32, PathBuf)> {
    let mut v = files.to_vec();
    v.sort_by_key(|(i, _)| *i);
    v
}

/// 流复制合并。快速合并要求所有输入的编码一致：由 [`crate::codec_probe::check`] 探测、在 `service::merge_service::quick::quick_merge`（上层壳）强制执行；
/// `index::plan` 的结构检查是第二道闸，管探测不到的轨数、轨道顺序与电影时基——任一对不上只会产出连索引都过不去的文件。
pub fn concat_copy(inputs: &[PathBuf], output: &Path) -> AppResult<(u64, usize)> {
    if inputs.is_empty() {
        return Err(AppError::Media("没有待合并的文件".into()));
    }
    if let Some(parent) = output.parent() {
        hongguo_common::offline_qa::qa_create_dir_all(parent)
            .map_err(|e| AppError::Io(e.to_string()))?;
    }

    let temp = hongguo_common::utils::path::temp_path_for(output);
    hongguo_common::offline_qa::qa_clear_for_replace(&temp)
        .map_err(|e| AppError::Io(e.to_string()))?;

    let prepared = prepare(inputs)?;
    let (size, count) = write_output(inputs, &temp, &prepared)?;

    hongguo_common::offline_qa::qa_rename(&temp, output).map_err(|e| {
        let _ = hongguo_common::offline_qa::qa_remove_file(&temp);
        AppError::Io(format!("原子替换失败: {e}"))
    })?;
    Ok((size, count))
}

/// 读出全部输入、规划索引、算出每个样本在产物里的偏移。
/// 解析只做一次：`write_output` 复用这里的结果，不重新读文件头——否则「规划时看到的内容」与「实际拷贝的内容」可能不是同一份。
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
    let wide = needs_wide_offsets(total_payload);

    // moov 长度只取决于样本数与偏移宽度，与偏移数值无关：先用占位偏移量一次长度，再带真实偏移重算一次
    let placeholders: Vec<Vec<u64>> = plan
        .tracks
        .iter()
        .map(|t| vec![0u64; t.sizes.len()])
        .collect();
    let moov_len = build_moov(&plan.mvhd, &plan.tracks, &placeholders, wide).len();
    let data_start = mdat_data_start(ftyp.len(), moov_len, wide);

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

    let moov = build_moov(&plan.mvhd, &plan.tracks, &offsets, wide);
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
    let mut out =
        hongguo_common::offline_qa::qa_create(temp).map_err(|e| AppError::Io(e.to_string()))?;
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
/// 按 box 声明的长度精确定位，不走「先读前 N 兆」那条路：moov 可能超过那个量，截断会让 `parse_boxes` **静默**少解析、产物「少几条样本表」而不报错。
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

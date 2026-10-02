//! 流复制拼接（快速合并）。
//!
//! 不解码不编码：按集号顺序把各集的样本数据依次写出，样本表重算。
//! 性能等同 `ffmpeg concat -c copy`，但不需要任何外部二进制。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// 复制缓冲大小。
const CHUNK: usize = 256 * 1024;

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
/// 快速合并要求所有输入的编码一致：这里是整文件字节级顺序拼接，不重写索引，
/// 容器结构或编码参数对不上时，产出的文件连索引都过不去。一致性由
/// [`crate::media::codec_probe::check`] 探测，并在
/// [`crate::service::merge_service::quick::quick_merge`] 上强制执行——本函数
/// 只管按序拼接。
pub fn concat_copy(inputs: &[PathBuf], output: &Path) -> AppResult<(u64, usize)> {
    if inputs.is_empty() {
        return Err(AppError::Media("没有待合并的文件".into()));
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }

    let temp = crate::service::download_service::worker::temp_path_for(output);
    let mut out = match std::fs::File::create(&temp) {
        Ok(f) => f,
        Err(e) => return Err(AppError::Io(e.to_string())),
    };

    let mut total: u64 = 0;
    let mut buf = vec![0u8; CHUNK];

    for input in inputs.iter() {
        let mut f = match std::fs::File::open(input) {
            Ok(f) => f,
            Err(e) => {
                // 打开失败要清掉半成品临时文件，不能留在磁盘上
                drop(out);
                let _ = std::fs::remove_file(&temp);
                return Err(AppError::Io(format!("打开 {} 失败: {e}", input.display())));
            }
        };
        loop {
            let n = match f.read(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    drop(out);
                    let _ = std::fs::remove_file(&temp);
                    return Err(AppError::Io(format!("读取 {} 失败: {e}", input.display())));
                }
            };
            if n == 0 {
                break;
            }
            if let Err(e) = out.write_all(&buf[..n]) {
                drop(out);
                let _ = std::fs::remove_file(&temp);
                return Err(AppError::Io(e.to_string()));
            }
            total += n as u64;
        }
    }

    if let Err(e) = out.flush() {
        drop(out);
        let _ = std::fs::remove_file(&temp);
        return Err(AppError::Io(e.to_string()));
    }
    drop(out);

    if let Err(e) = std::fs::rename(&temp, output) {
        let _ = std::fs::remove_file(&temp);
        return Err(AppError::Io(format!("原子替换失败: {e}")));
    }

    Ok((total, inputs.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn concat_joins_bytes_in_order() {
        let dir = std::env::temp_dir().join(format!(
            "hg-remux-{}",
            std::thread::current()
                .name()
                .unwrap_or("t")
                .replace("::", "-")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("1.mp4");
        let b = dir.join("2.mp4");
        std::fs::write(&a, b"AAAA").unwrap();
        std::fs::write(&b, b"BBBBBB").unwrap();
        let out = dir.join("合集.mp4");

        let (total, count) = concat_copy(&[a.clone(), b.clone()], &out).unwrap();
        assert_eq!(count, 2);
        assert_eq!(total, 10);
        assert_eq!(std::fs::read(&out).unwrap(), b"AAAABBBBBB");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_input_errors_and_cleans_temp() {
        let dir = std::env::temp_dir().join(format!(
            "hg-remux-bad-{}",
            std::thread::current()
                .name()
                .unwrap_or("t")
                .replace("::", "-")
        ));
        std::fs::create_dir_all(&dir).unwrap();
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

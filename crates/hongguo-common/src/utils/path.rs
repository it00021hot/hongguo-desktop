//! 落盘临时文件的路径约定：下载、转码（`hongguo-media` 的 transcode / remux）与 ffmpeg 执行三条链路都走「先写临时文件、成功后再 rename」。
//!
//! 临时名的算法必须在同一处，否则同一次操作的不同链路会得出不同名字，互相把对方的产物当输入或残留清理掉。

use std::path::{Path, PathBuf};

/// 落盘临时文件名：`xxx.mp4` → `xxx.enc.tmp`（`.enc` 承接历史语义保留不变，让已有残留临时文件仍可识别）。无扩展名/无文件名时退化为 `download.enc.tmp`——与输出文件同目录同卷，rename 才是原子的。
pub fn temp_path_for(output: &Path) -> PathBuf {
    let stem = output
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());
    output.with_file_name(format!("{stem}.enc.tmp"))
}

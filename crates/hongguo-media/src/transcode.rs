//! 平台归档转码结果与文件读取。HEVC 封装在 [`crate::mux`]。

use std::path::Path;

use hongguo_common::{AppError, AppResult};

/// 转码结果。
#[derive(Debug, Clone)]
pub struct TranscodeResult {
    /// 转码产物的路径
    pub output_path: String,
    /// 归档编码后端；缓存产物在当前设备无编码器时为 `None`。
    pub backend: Option<crate::Backend>,
}

/// 从文件读一段字节（平台后端按样本偏移喂解码器时用）。
///
/// 与 [`crate::mux::with_audio`] 同理对外可见：平台后端在另一个 crate 里。
pub fn read_range(path: &Path, offset: u64, size: u64) -> AppResult<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).map_err(|e| AppError::Io(e.to_string()))?;
    f.seek(SeekFrom::Start(offset))
        .map_err(|e| AppError::Io(e.to_string()))?;
    let mut buf = vec![0u8; size as usize];
    f.read_exact(&mut buf)
        .map_err(|e| AppError::Media(format!("读取样本失败: {e}")))?;
    Ok(buf)
}

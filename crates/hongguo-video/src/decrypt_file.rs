//! 落盘解密：整文件读入 → 原地覆盖视频轨样本的密文 → 整体写出，随后 [`super::deprotect`] 摘掉加密标记（否则产物仍被当成加密流）。
//! ⚠️ 不要改成「重排文件 + 重建 moov」那套看似省内存的写法：那必须同步改写 `stco` / `stsz`，漏一处 `stco` 播放器就读到错位数据且**不报错**。
//! 整文件进内存换来「布局不变」（音轨、`stco`、采样表全是现成的），单集短剧 10~100 MB 代价可接受。

use std::path::Path;

use hongguo_common::{AppError, AppResult};

/// 解密一个 MP4 文件：`src` 为加密源文件（通常是 `.enc.tmp`），`dst` 为解密后的成品，`key` 为 AES-128 密钥，返回成品字节数。
pub fn decrypt_mp4_file(src: &Path, dst: &Path, key: &[u8; 16]) -> AppResult<u64> {
    let data = std::fs::read(src).map_err(|e| AppError::Io(format!("读取源文件失败: {e}")))?;
    let plain = super::decrypt_buffer::decrypt_mp4_buffer(&data, key)?;
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }
    hongguo_common::file_io::write(dst, &plain).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(plain.len() as u64)
}

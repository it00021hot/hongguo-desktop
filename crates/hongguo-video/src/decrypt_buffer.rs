//! 内存解密（在线播放用）：与 [`super::decrypt_file`] 的区别只在数据源与产出。
//! 关键约定（与现版 JS 逐字对齐，**不要按标准 CENC 想当然改**）：IV 是 8 字节（从 `senc` 按 8 字节步长取，读成 16 会整体错位）；
//! 没有 IV 的样本不解密（本项目不解析 `tenc`，`default_iv` 恒全 0，兜底只产噪声）；所有有 IV 的轨都要解（漏音轨 = 有画面没声音）。

use super::sample_table::collect_tracks;
use crate::crypto::cenc;
use hongguo_common::{AppError, AppResult};

/// 在内存中解密整个 MP4。
pub fn decrypt_mp4_buffer(data: &[u8], key: &[u8; 16]) -> AppResult<Vec<u8>> {
    let tracks = collect_tracks(data)?;
    if tracks.is_empty() {
        return Err(AppError::Decrypt("没有可用轨道".into()));
    }

    let mut out = data.to_vec();
    for track in &tracks {
        // 没有逐样本 IV 就整轨跳过，与现版一致
        if track.sample_ivs.is_empty() {
            continue;
        }
        if track.samples.is_empty() {
            return Err(AppError::Decrypt(format!(
                "轨道 {} 没有样本",
                track.track_id
            )));
        }

        for (i, &(offset, size)) in track.samples.iter().enumerate() {
            let Some(&iv) = track.sample_ivs.get(i) else {
                continue;
            };
            let start = offset as usize;
            let end = start + size as usize;
            if end > out.len() {
                return Err(AppError::Decrypt(format!(
                    "轨道 {} 样本 {i} 越界",
                    track.track_id
                )));
            }

            let mut decryptor = cenc::new_decryptor(key, &iv);
            cenc::decrypt_sample(&mut decryptor, &mut out[start..end]);
        }
    }

    // 采样数据解开只是第一步：入口还叫 `encv`、`sinf`/`tenc` 还在，播放器会当加密流拒绝播放，必须一并摘掉加密标记
    super::deprotect::deprotect(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garbage_input_errors_not_panics() {
        let result = decrypt_mp4_buffer(&[0xffu8; 500], &[0u8; 16]);
        assert!(result.is_err());
    }

    #[test]
    fn empty_input_errors() {
        assert!(decrypt_mp4_buffer(&[], &[0u8; 16]).is_err());
    }

    #[test]
    fn no_iv_leaves_data_untouched() {
        let r = decrypt_mp4_buffer(&[0u8; 64], &[0u8; 16]);
        assert!(r.is_err(), "无法解析轨道时应报错");
    }
}

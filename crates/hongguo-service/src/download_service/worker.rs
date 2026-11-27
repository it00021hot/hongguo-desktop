//! 单集执行：取流 → 流式下载 → 解密 → 落盘。
//! 防损：先写 `.enc.tmp`，解密成功后才改名 `.mp4`，中途断网不留残缺文件。
//! 设置（尤其代理）由调用方传入：worker 自己拿默认值会让用户配的代理失效。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::StreamExt;

use crate::download_service::events::ProgressThrottle;
use hongguo_common::model::Settings;
use hongguo_common::utils::path::temp_path_for;
use hongguo_common::{AppError, AppResult};

/// 下载与解密的进度回调。回调用 `Arc<dyn Fn>` 持有而非借用：
/// worker 在 spawn 里跑，借用的闭包会让整个 future 变成非 `Send`。
pub struct ProgressSink {
    pub throttle: Arc<ProgressThrottle>,
    pub on_progress: Arc<dyn Fn(u64, u64) + Send + Sync>,
}

impl ProgressSink {
    /// 报告进度（内部已做节流判断）。
    pub fn report(&self, id: &str, downloaded: u64, total: u64) {
        let percent = if total == 0 {
            0.0
        } else {
            (downloaded as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
        };
        if self.throttle.should_send(id, percent) {
            (self.on_progress)(downloaded, total);
        }
    }
}

/// 单集下载的输入。
pub struct EpisodeDownload<'a> {
    pub id: String,
    /// 直链（已签名）
    pub video_url: String,
    pub output_path: PathBuf,
    pub validate_bytevc2: bool,
    /// 是否为 CENC 加密流
    pub encrypted: bool,
    /// 加密流使用的密钥材料（`spade_a` 原始值）
    pub key_material: Vec<u8>,
    /// 用户设置（代理、并发等）
    pub settings: Settings,
    /// 取消标志：一键暂停时置位。由调度器共享持有，这里只借用。
    pub cancelled: &'a AtomicBool,
}

/// 下载并解密一集。
///
/// **取流时不带 Referer** —— 视频 CDN 对带 Referer 的请求直接 403。
pub async fn download_episode(
    episode: &EpisodeDownload<'_>,
    sink: &ProgressSink,
) -> AppResult<u64> {
    if let Some(parent) = episode.output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
    }

    let temp_path = temp_path_for(&episode.output_path);
    let client = hongguo_api::client::build_client(&episode.settings.proxy)?;

    let resp = hongguo_api::client::get_video_stream(&client, &episode.video_url).await?;

    let total = resp.content_length().unwrap_or(0);
    let mut file =
        hongguo_common::file_io::create(&temp_path).map_err(|e| AppError::Io(e.to_string()))?;

    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if episode.cancelled.load(Ordering::SeqCst) {
            drop(file);
            let _ = hongguo_common::file_io::remove_file(&temp_path);
            return Err(AppError::Cancelled);
        }
        let chunk = chunk.map_err(|e| AppError::Network(e.without_url().to_string()))?;
        std::io::Write::write_all(&mut file, &chunk).map_err(|e| AppError::Io(e.to_string()))?;
        downloaded += chunk.len() as u64;
        sink.report(&episode.id, downloaded, total);
    }
    use std::io::Write;
    file.flush().map_err(|e| AppError::Io(e.to_string()))?;
    drop(file);

    finalize(&temp_path, &episode.output_path, episode)?;

    let size = std::fs::metadata(&episode.output_path)
        .map(|m| m.len())
        .unwrap_or(0);
    sink.report(&episode.id, size, size);
    Ok(size)
}

/// 收尾：密流解密后写出成品，明流直接改名。两条路径都要清掉临时文件：
/// 解密失败时留着它会被下次重试当成「上次下了一半」复用；解密成功时它已没用
///（CENC 原地加密，密文明文一样大，不删看着像下了两份）；明流 rename 后删除是 no-op。
fn finalize(temp: &Path, output: &Path, episode: &EpisodeDownload) -> AppResult<()> {
    let staged = episode
        .validate_bytevc2
        .then(|| output.with_extension("validated.mp4.tmp"));
    let destination = staged.as_deref().unwrap_or(output);
    let result: AppResult<()> = if episode.encrypted {
        match hongguo_video::crypto::key_derive::derive_key(&episode.key_material) {
            Ok(key) => {
                hongguo_video::decrypt_file::decrypt_mp4_file(temp, destination, &key).map(|_| ())
            }
            Err(e) => Err(e),
        }
    } else {
        hongguo_common::file_io::rename(temp, destination)
            .map_err(|e| AppError::Io(format!("改名失败: {e}")))
    };

    let _ = hongguo_common::file_io::remove_file(temp);
    let Some(staged) = staged else {
        return result;
    };
    let result = result.and_then(|()| {
        let path = staged
            .to_str()
            .ok_or_else(|| AppError::InvalidArgs("ByteVC2 下载路径无效".into()))?;
        let codec = crate::play_service::local::local_playback_codec(path)?;
        if !hongguo_bytevc2::is_bytevc2(&codec) {
            return Err(AppError::CodecUnsupported(codec));
        }
        hongguo_common::file_io::rename(&staged, output)
            .map_err(|e| AppError::Io(format!("改名失败: {e}")))
    });
    let _ = hongguo_common::file_io::remove_file(&staged);
    result
}

/// 清理残留的临时文件。
pub fn cleanup_temp(output: &Path) {
    let _ = hongguo_common::file_io::remove_file(&temp_path_for(output));
}

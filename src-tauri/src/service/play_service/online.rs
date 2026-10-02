//! 在线播放：取流 → 解密 → 推进内存流。
//!
//! 数据面在 [`crate::protocol::stream`]，这里只负责把明文推进去。
//!
//! ⚠️ 为什么不边下边解密：CENC 样本解密要先读 moov 里的样本表才能定位每个
//!    样本的字节区间，moov 又可能在文件尾部。所以现在先整集取回再解密，
//!    一次性交给 `serve`。**首帧要等整集下完**，比现版的渐进式播放慢，
//!    但结果字节与落盘下载完全一致。

use futures_util::StreamExt;

use crate::domain::model::Settings;
use crate::error::{AppError, AppResult};
use crate::protocol::stream::StreamCache;

/// 全局流缓存。
pub fn cache() -> &'static StreamCache {
    use std::sync::OnceLock;
    static CACHE: OnceLock<StreamCache> = OnceLock::new();
    CACHE.get_or_init(StreamCache::default)
}

/// 准备在线播放，返回 `hongguo-stream://<vid>`。
///
/// 幂等：同一个 vid 重复调用不会重复下载，已就绪的直接返回 URL。
pub async fn prepare(vid: &str, settings: Settings) -> AppResult<String> {
    let vid = vid.trim();
    if vid.is_empty() {
        return Err(AppError::InvalidArgs("缺少 vid".into()));
    }
    let c = cache();
    if c.get(vid).is_some() {
        return Ok(c.stream_url(vid));
    }

    // 先占位再取流：占位期间重复调用会走上面的幂等分支，避免并发下重复下载
    c.entry(vid);

    let play = match crate::domain::api::play_url::fetch_play_url(vid, &settings.proxy).await {
        Ok(play) => play,
        Err(e) => {
            c.remove(vid);
            return Err(e);
        }
    };

    let url = c.stream_url(vid);
    let owned = vid.to_string();
    // `cache()` 返回 `&'static StreamCache`，可以直接带进 spawn 的 future
    let c = cache();
    tokio::spawn(async move {
        if let Err(e) = fill(c, &owned, &play, &settings).await {
            log::warn!("[Online] 填充 {owned} 失败: {e}");
            c.remove(&owned);
        }
    });

    Ok(url)
}

/// 下载一集、解密、整段推进缓存。
async fn fill(
    c: &StreamCache,
    vid: &str,
    play: &crate::domain::api::play_url::PlayInfo,
    settings: &Settings,
) -> AppResult<()> {
    let plain = fetch_plain(play, settings).await?;

    // 先给尺寸再给内容：serve 见到 size > 0 才开始供给
    c.set_size(vid, plain.len() as u64);
    c.append(vid, &plain);
    log::info!("[Online] {vid} 就绪，{} 字节", plain.len());
    Ok(())
}

/// 取一集并解密成明文字节（CDN 下载 + CENC 解密）。
///
/// 在线播放与兼容转码都要这一步，抽出来避免两份「取流 + 解密」逻辑各自漂移。
pub async fn fetch_plain(
    play: &crate::domain::api::play_url::PlayInfo,
    settings: &Settings,
) -> AppResult<Vec<u8>> {
    let client = crate::domain::api::client::build_client(&settings.proxy)?;

    // CDN 对带 Referer 的请求直接 403，只带 App UA
    let resp = client
        .get(&play.url)
        .header("User-Agent", crate::signer::VIDEO_UA)
        .send()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(AppError::Network(format!(
            "取流失败: HTTP {}",
            resp.status()
        )));
    }

    let mut body: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::Network(e.to_string()))?;
        body.extend_from_slice(&chunk);
    }

    if play.encrypted {
        let key = crate::domain::crypto::key_derive::derive_key(&play.key_material)?;
        crate::domain::mp4::decrypt_buffer::decrypt_mp4_buffer(&body, &key)
    } else {
        Ok(body)
    }
}

/// 清空在线缓存。
pub fn clear() -> usize {
    let (count, _) = cache().status();
    cache().clear();
    count
}

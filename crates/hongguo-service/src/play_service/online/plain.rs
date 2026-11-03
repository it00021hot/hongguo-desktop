//! 整集抓取：CDN 下载 + CENC 解密成明文字节（在线播放与兼容转码共用）。

use futures_util::StreamExt;

use hongguo_common::model::Settings;
use hongguo_common::{AppError, AppResult};

/// 取一集并解密成明文（CDN 下载 + CENC 解密），在线播放与兼容转码共用一份实现避免漂移。
pub async fn fetch_plain(
    play: &hongguo_api::play_url::PlayInfo,
    settings: &Settings,
) -> AppResult<Vec<u8>> {
    fetch_plain_with(play, settings, &|_, _, _| {}).await
}

/// 带进度回调的取流+解密。
pub async fn fetch_plain_with(
    play: &hongguo_api::play_url::PlayInfo,
    settings: &Settings,
    progress: &(dyn Fn(u64, u64, &str) + Sync),
) -> AppResult<Vec<u8>> {
    let client = hongguo_api::client::build_client(&settings.proxy)?;

    // 首选裸 UA；个别 CDN 节点反过来要求 Referer，403 时在里面补上重试
    let resp = hongguo_api::client::get_video_stream(&client, &play.url).await?;

    // 拿不到 Content-Length 就发不了百分比，但「开始动了」这件事仍要告诉界面
    let total = resp.content_length().unwrap_or(0);
    progress(0, total, "downloading");

    let mut body: Vec<u8> = Vec::with_capacity(total as usize);
    let mut received = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::Network(e.to_string()))?;
        received += chunk.len() as u64;
        body.extend_from_slice(&chunk);
        progress(received, total, "downloading");
    }
    progress(received, total.max(received), "decrypting");

    if play.encrypted {
        let key = hongguo_video::crypto::key_derive::derive_key(&play.key_material)?;
        hongguo_video::decrypt_buffer::decrypt_mp4_buffer(&body, &key)
    } else {
        Ok(body)
    }
}

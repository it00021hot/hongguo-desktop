//! Reuse only the exact media resource that filled the online cache.
use hongguo_api::play_url::PlayInfo;
use hongguo_common::{AppError, AppResult};
use parking_lot::Mutex;
use std::{collections::HashMap, sync::LazyLock};

#[derive(Clone, PartialEq)]
struct Identity {
    url: String,
    codec: String,
    encrypted: bool,
    key_material: Vec<u8>,
    file_hash: Option<String>,
}
impl From<&PlayInfo> for Identity {
    fn from(play: &PlayInfo) -> Self {
        Self {
            url: play.url.clone(),
            codec: play.codec.clone(),
            encrypted: play.encrypted,
            key_material: play.key_material.clone(),
            file_hash: play.file_hash.clone(),
        }
    }
}
static IDENTITIES: LazyLock<Mutex<HashMap<(String, u32), Identity>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn remember(vid: &str, play: &PlayInfo) {
    let mut identities = IDENTITIES.lock();
    identities.retain(|(vid, definition), _| hongguo_stream::cache().exists(vid, *definition));
    identities.insert((vid.to_owned(), play.definition), Identity::from(play));
}

pub(in crate::play_service) async fn plain(
    vid: &str,
    play: &PlayInfo,
) -> AppResult<Option<Vec<u8>>> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let key = (vid.to_owned(), play.definition);
    let expected = Identity::from(play);
    if IDENTITIES.lock().get(&key) != Some(&expected)
        || !hongguo_stream::cache().exists(vid, play.definition)
    {
        return Ok(None);
    }
    struct CancelOnDrop(Arc<AtomicBool>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let guard = CancelOnDrop(Arc::new(AtomicBool::new(false)));
    let cancelled = guard.0.clone();
    let read_key = key.clone();
    let result = crate::runtime::spawn_blocking(move || {
        hongguo_stream::read_range_cancellable(&read_key.0, read_key.1, 0, None, move || {
            cancelled.load(Ordering::Acquire)
        })
    })
    .await
    .map_err(|error| AppError::Internal(format!("读取兼容播放缓存任务失败: {error}")))?;
    match result {
        Ok(chunk) if IDENTITIES.lock().get(&key) == Some(&expected) => Ok(Some(chunk.bytes)),
        Ok(_)
        | Err(
            hongguo_stream::StreamReadError::Missing { .. }
            | hongguo_stream::StreamReadError::Replaced,
        ) => Ok(None),
        Err(error) => Err(AppError::Media(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vid() -> String {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        format!(
            "native-compat-test-{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }
    #[tokio::test]
    async fn matching_resource_reuses_the_filled_stream_without_a_second_download() {
        let vid = vid();
        let play = PlayInfo {
            url: "https://media.example.test/one".into(),
            codec: "bytevc2".into(),
            definition: 720,
            ..Default::default()
        };
        let cache = hongguo_stream::cache();
        cache.store(&vid, 720, b"decoded mp4 bytes");
        remember(&vid, &play);
        let bytes = plain(&vid, &play).await.unwrap();
        cache.remove(&vid, 720);
        assert_eq!(bytes, Some(b"decoded mp4 bytes".to_vec()));
    }
    #[tokio::test]
    async fn same_definition_is_not_enough_when_codec_resource_or_key_changes() {
        let vid = vid();
        let play = PlayInfo {
            url: "https://media.example.test/one".into(),
            codec: "bytevc2".into(),
            definition: 720,
            ..Default::default()
        };
        let cache = hongguo_stream::cache();
        cache.store(&vid, 720, b"wrong resource");
        remember(&vid, &play);
        for changed in [
            PlayInfo {
                codec: "h265".into(),
                ..play.clone()
            },
            PlayInfo {
                url: "https://media.example.test/two".into(),
                ..play.clone()
            },
            PlayInfo {
                key_material: vec![1, 2],
                ..play.clone()
            },
        ] {
            assert_eq!(plain(&vid, &changed).await.unwrap(), None);
        }
        cache.remove(&vid, 720);
    }
}

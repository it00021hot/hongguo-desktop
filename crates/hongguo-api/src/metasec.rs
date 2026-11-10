//! MetaSec v5 启动交换与请求签名。

mod device;
mod materials;
mod request;
pub mod sdi;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use hongguo_common::{AppError, AppResult};
use hongguo_signer::device::DeviceProfile;
use hongguo_signer::medusa_v5::RequestSigner;
use hongguo_signer::primitives::sm3;
use parking_lot::Mutex;
use tokio::sync::OnceCell;

pub(crate) use request::sign_playback;

pub trait SessionStore: Send + Sync + 'static {
    fn load(&self, profile_key: &str) -> AppResult<Option<sdi::SdiToken>>;
    fn save(&self, profile_key: &str, token: &sdi::SdiToken) -> AppResult<()>;
}

struct AppConfig {
    materials_cache: PathBuf,
    sessions: Arc<dyn SessionStore>,
}

static APP: OnceLock<AppConfig> = OnceLock::new();

/// 指定应用私有目录及 SDK 会话存储。
pub fn configure_app(database_path: &Path, sessions: Arc<dyn SessionStore>) {
    let _ = APP.set(AppConfig {
        materials_cache: database_path.with_file_name(materials::CACHE_NAME),
        sessions,
    });
}

struct MetaSec {
    signer: RequestSigner,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

#[derive(Default)]
struct Session {
    initial: OnceCell<sdi::SdiToken>,
    refresh: OnceCell<sdi::SdiToken>,
}

impl MetaSec {
    async fn required(client: &reqwest::Client) -> AppResult<&'static Self> {
        static INSTANCE: OnceCell<MetaSec> = OnceCell::const_new();
        INSTANCE
            .get_or_try_init(|| async {
                let cache = APP.get().map(|app| app.materials_cache.as_path());
                let signer = materials::load(client, cache).await?;
                Ok(Self {
                    signer,
                    sessions: Mutex::default(),
                })
            })
            .await
    }

    fn session(&self, device: &DeviceProfile) -> AppResult<(String, Arc<Session>)> {
        let (did, iid) = (device.get("device_id"), device.get("iid"));
        if [did, iid].iter().any(|value| {
            value.is_empty() || *value == "0" || !value.bytes().all(|b| b.is_ascii_digit())
        }) {
            return Err(AppError::InvalidArgs(
                "MetaSec requires an existing registered device profile".into(),
            ));
        }
        let key = hex::encode(sm3(format!("{did}:{iid}:{}", device.get("cdid")).as_bytes()));
        let session = self.sessions.lock().entry(key.clone()).or_default().clone();
        Ok((key, session))
    }
}

//! MetaSec v5 启动交换与请求签名。

mod device;
mod materials;
mod request;
pub mod sdi;

use std::collections::HashMap;
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

static APP: OnceLock<Arc<dyn SessionStore>> = OnceLock::new();

pub fn configure_app(sessions: Arc<dyn SessionStore>) {
    let _ = APP.set(sessions);
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
    async fn required() -> AppResult<&'static Self> {
        static INSTANCE: OnceCell<MetaSec> = OnceCell::const_new();
        INSTANCE
            .get_or_try_init(|| async {
                let signer = tokio::task::spawn_blocking(materials::load)
                    .await
                    .map_err(|_| AppError::Internal("MetaSec material task failed".into()))??;
                Ok(Self {
                    signer,
                    sessions: Mutex::default(),
                })
            })
            .await
    }

    fn session(&self, device: &DeviceProfile) -> AppResult<(String, Arc<Session>)> {
        if !crate::register::has_registered_ids(device) {
            return Err(AppError::InvalidArgs(
                "MetaSec requires an existing registered device profile".into(),
            ));
        }
        let (did, iid) = (device.get("device_id"), device.get("iid"));
        let key = hex::encode(sm3(format!("{did}:{iid}:{}", device.get("cdid")).as_bytes()));
        let session = self.sessions.lock().entry(key.clone()).or_default().clone();
        Ok((key, session))
    }
}

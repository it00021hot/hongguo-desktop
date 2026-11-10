//! MetaSec v5 启动交换与请求签名。

mod device;
mod request;
pub mod sdi;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use hongguo_common::{AppError, AppResult};
use hongguo_signer::device::DeviceProfile;
use hongguo_signer::medusa_v5::{Field13, License, RequestSigner};
use hongguo_signer::primitives::sm3;
use parking_lot::Mutex;
use tokio::sync::OnceCell;

pub(crate) use request::sign_playback;

/// Whether the app has loaded the local SDK material for the v5 playback routes.
pub fn playback_configured() -> AppResult<bool> {
    MetaSec::configured().map(|config| config.is_some())
}

pub trait SessionStore: Send + Sync + 'static {
    fn load(&self, profile_key: &str) -> AppResult<Option<sdi::SdiToken>>;
    fn save(&self, profile_key: &str, token: &sdi::SdiToken) -> AppResult<()>;
}

struct AppConfig {
    local_config: PathBuf,
    sessions: Arc<dyn SessionStore>,
}

static APP: OnceLock<AppConfig> = OnceLock::new();

/// Register the app's private SDK configuration and the corresponding session store.
pub fn configure_app(local_config: PathBuf, sessions: Arc<dyn SessionStore>) {
    let _ = APP.set(AppConfig {
        local_config,
        sessions,
    });
}

#[derive(serde::Deserialize)]
struct LocalConfig {
    image: PathBuf,
    license: String,
    environment_label: String,
}

impl LocalConfig {
    fn load() -> Result<Option<Self>, &'static str> {
        if let Some(image) = std::env::var_os("HONGGUO_METASEC_IMAGE") {
            return Ok(Some(Self {
                image: image.into(),
                license: std::env::var("HONGGUO_METASEC_LICENSE")
                    .map_err(|_| "missing MetaSec license")?,
                environment_label: std::env::var("HONGGUO_METASEC_ENV_LABEL")
                    .map_err(|_| "missing MetaSec environment label")?,
            }));
        }
        let Some(app) = APP.get() else {
            return Ok(None);
        };
        let config = match std::fs::read(&app.local_config) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("cannot read the local MetaSec configuration"),
        };
        let config: Self =
            serde_json::from_slice(&config).map_err(|_| "invalid local MetaSec configuration")?;
        if !config.image.is_absolute() {
            return Err("the local MetaSec ELF path must be absolute");
        }
        Ok(Some(config))
    }
}

struct MetaSec {
    signer: RequestSigner,
    environment_label: String,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

#[derive(Default)]
struct Session {
    initial: OnceCell<sdi::SdiToken>,
    refresh: OnceCell<sdi::SdiToken>,
}

impl MetaSec {
    fn configured() -> AppResult<Option<&'static Self>> {
        static INSTANCE: OnceLock<Result<Option<MetaSec>, &'static str>> = OnceLock::new();
        match INSTANCE.get_or_init(Self::load) {
            Ok(instance) => Ok(instance.as_ref()),
            Err(message) => Err(AppError::InvalidArgs((*message).into())),
        }
    }

    fn load() -> Result<Option<Self>, &'static str> {
        let Some(config) = LocalConfig::load()? else {
            return Ok(None);
        };
        let image =
            std::fs::read(config.image).map_err(|_| "cannot read the configured MetaSec ELF")?;
        let image = Field13::from_image(image).map_err(|_| "unsupported MetaSec ELF")?;
        let license = License::decode(&config.license).ok_or("invalid MetaSec license")?;
        let environment_label = config.environment_label;
        if environment_label.len() != 20
            || !environment_label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err("invalid MetaSec environment label");
        }
        Ok(Some(Self {
            signer: RequestSigner::new(image, license),
            environment_label,
            sessions: Mutex::default(),
        }))
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

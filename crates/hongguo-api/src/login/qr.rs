//! A user-confirmed QR attempt against the official Hongguo web passport.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use hongguo_common::{AppError, AppResult, model::ProxyConfig};
use reqwest::{
    Client, Response, Url,
    cookie::{CookieStore, Jar},
    header,
};
use serde_json::Value;

use super::{PassportUser, parse_user};

const ORIGIN: &str = "https://hongguoduanju.com";
const HOME: &str = "https://hongguoduanju.com/";
const SITE: &str = "hongguoduanju.com";
const RESPONSE_LIMIT: usize = 2 * 1024 * 1024;
const IMAGE_LIMIT: usize = 1024 * 1024;
const TOKEN_LIMIT: usize = 8192;
const MAX_LIFETIME: Duration = Duration::from_secs(10 * 60);
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
const PASSPORT_PARAMS: &[(&str, &str)] = &[
    ("aid", "513755"),
    ("passport_jssdk_version", "3.4.10"),
    ("passport_jssdk_type", "normal"),
    ("account_sdk_source", "web"),
    ("is_from_ttaccountsdk", "1"),
    ("language", "zh"),
];

pub struct Attempt {
    client: Client,
    cookies: Arc<Jar>,
    token: String,
    expires: Instant,
}

pub enum Status {
    Waiting,
    Scanned,
    Cancelled,
    Expired,
    Rotated(String),
    Confirmed { user: PassportUser, cookies: String },
}

impl Attempt {
    pub async fn start(proxy: &ProxyConfig) -> AppResult<(Self, String)> {
        let cookies = Arc::new(Jar::default());
        let builder = Client::builder()
            .user_agent(USER_AGENT)
            .cookie_provider(cookies.clone())
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(12));
        #[cfg(target_os = "android")]
        let builder =
            builder.use_preconfigured_tls(hongguo_common::utils::tls::webpki_tls_config()?);
        let builder = if let Some(proxy) = proxy.resolved() {
            builder.proxy(reqwest::Proxy::all(proxy).map_err(|_| failure("Invalid proxy"))?)
        } else {
            builder.no_proxy()
        };
        let mut attempt = Self {
            client: builder
                .build()
                .map_err(|_| failure("Cannot initialize QR login"))?,
            cookies,
            token: String::new(),
            expires: Instant::now(),
        };
        let data = attempt
            .json("/passport/web/get_qrcode/", &[("next", HOME)])
            .await?;
        let image = attempt
            .set_challenge(&data)
            .ok_or_else(|| failure("Invalid QR challenge"))?;
        Ok((attempt, image))
    }

    pub async fn poll(&mut self) -> AppResult<Status> {
        if Instant::now() >= self.expires {
            return Ok(Status::Expired);
        }
        let token = self.token.clone();
        let data = self
            .json(
                "/passport/web/check_qrconnect/",
                &[("next", HOME), ("token", &token)],
            )
            .await?;
        let status = data
            .get("status")
            .ok_or_else(|| failure("Missing QR status"))?;
        match (status.as_u64(), status.as_str()) {
            (Some(1), _) | (_, Some("1" | "new" | "pending")) => Ok(Status::Waiting),
            (Some(2), _) | (_, Some("2" | "scanned")) => Ok(Status::Scanned),
            (Some(4), _) | (_, Some("4" | "cancelled")) => Ok(Status::Cancelled),
            (Some(5), _) | (_, Some("5" | "expired")) => Ok(self
                .set_challenge(&data)
                .map_or(Status::Expired, Status::Rotated)),
            (Some(3), _) | (_, Some("3" | "confirmed")) => self.complete(&data).await,
            _ => Err(failure("Unrecognized QR status")),
        }
    }

    fn set_challenge(&mut self, data: &Value) -> Option<String> {
        let token = data.get("token")?.as_str()?;
        let png = data.get("qrcode")?.as_str()?.trim();
        let png = png.strip_prefix("data:image/png;base64,").unwrap_or(png);
        if token.is_empty()
            || token.len() > TOKEN_LIMIT
            || png.len() > IMAGE_LIMIT
            || !png.starts_with("iVBORw0KGgo")
            || !png
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || b"+/=".contains(&ch))
        {
            return None;
        }
        if self.token == token {
            return None;
        }
        let expires = data.get("expire_time")?;
        let epoch = expires
            .as_i64()
            .or_else(|| expires.as_str()?.parse().ok())?;
        let seconds = epoch.checked_sub(chrono::Utc::now().timestamp())?;
        let lifetime = Duration::from_secs(u64::try_from(seconds).ok()?).min(MAX_LIFETIME);
        if lifetime.is_zero() {
            return None;
        }
        self.expires = Instant::now() + lifetime;
        self.token = token.to_owned();
        Some(format!("data:image/png;base64,{png}"))
    }

    async fn complete(&mut self, data: &Value) -> AppResult<Status> {
        if let Some(redirect) = data
            .get("redirect_url")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
        {
            let mut target = Url::parse(ORIGIN)
                .and_then(|origin| origin.join(redirect))
                .map_err(|_| failure("Invalid passport redirect"))?;
            let mut complete = false;
            for _ in 0..5 {
                let response = self.get(target.clone()).await?;
                if response.status().is_success() {
                    complete = true;
                    break;
                }
                if !response.status().is_redirection() {
                    break;
                }
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| failure("Invalid passport redirect"))?;
                target = target
                    .join(location)
                    .map_err(|_| failure("Invalid passport redirect"))?;
            }
            if !complete {
                return Err(failure("Passport confirmation did not complete"));
            }
        }
        let identity = self.json("/passport/account/info/v2/", &[]).await?;
        let mut user = parse_user(&identity, "");
        if let Some(exact) = identity
            .get("user_id_str")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        {
            if !user.user_id.is_empty() && user.user_id != exact {
                return Err(failure("Passport returned conflicting account IDs"));
            }
            user.user_id = exact.to_owned();
        }
        if user.name.is_empty() {
            user.name = identity
                .get("screen_name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
        }
        if user
            .user_id
            .parse::<u64>()
            .ok()
            .filter(|id| *id != 0)
            .is_none()
        {
            return Err(failure("Passport did not verify a signed-in account"));
        }
        let origin = Url::parse(ORIGIN).map_err(|_| failure("Invalid passport URL"))?;
        let header = self
            .cookies
            .cookies(&origin)
            .ok_or_else(|| failure("Passport did not issue a session"))?;
        let header = header
            .to_str()
            .map_err(|_| failure("Invalid passport session"))?;
        let mut has_session = false;
        let cookies = header
            .split("; ")
            .filter_map(|pair| {
                let (name, value) = pair.split_once('=')?;
                if value.is_empty() {
                    return None;
                }
                if matches!(name, "sessionid" | "sessionid_ss") {
                    has_session = true;
                }
                matches!(
                    name,
                    "sessionid"
                        | "sessionid_ss"
                        | "sid_guard"
                        | "sid_tt"
                        | "uid_tt"
                        | "uid_tt_ss"
                        | "passport_auth_status"
                        | "passport_auth_status_ss"
                )
                .then_some(pair)
            })
            .collect::<Vec<_>>()
            .join("; ");
        if !has_session {
            return Err(failure("Passport did not issue a session"));
        }
        Ok(Status::Confirmed { user, cookies })
    }

    async fn json(&mut self, path: &str, query: &[(&str, &str)]) -> AppResult<Value> {
        let mut url =
            Url::parse(&format!("{ORIGIN}{path}")).map_err(|_| failure("Invalid passport URL"))?;
        url.query_pairs_mut()
            .extend_pairs(PASSPORT_PARAMS)
            .extend_pairs(query);
        let response = self.get(url).await?;
        if !response.status().is_success() {
            return Err(failure("Passport is unavailable"));
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| failure("Interrupted passport response"))?;
            if chunk.len() > RESPONSE_LIMIT - bytes.len() {
                return Err(failure("Passport response too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| failure("Invalid passport response"))?;
        if value.get("message").and_then(Value::as_str) != Some("success")
            || super::error_code_of(&value) != 0
        {
            return Err(failure("Passport rejected the QR request"));
        }
        value
            .get("data")
            .filter(|data| data.is_object())
            .cloned()
            .ok_or_else(|| failure("Missing passport data"))
    }

    async fn get(&self, url: Url) -> AppResult<Response> {
        if url.scheme() != "https"
            || url.host_str() != Some(SITE)
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(failure("Passport redirect left the official origin"));
        }
        self.client
            .get(url)
            .header(header::REFERER, HOME)
            .header(header::ORIGIN, ORIGIN)
            .send()
            .await
            .map_err(|_| failure("Cannot reach passport"))
    }
}

fn failure(message: &str) -> AppError {
    AppError::Auth(message.to_owned())
}

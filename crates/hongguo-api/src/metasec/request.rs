use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use hongguo_common::{AppError, AppResult};
use hongguo_signer::device::{DeviceProfile, anonymous_cookie, fingerprint_for_cdid};
use hongguo_signer::medusa_v5::{
    LICENSE_CHANNEL, RequestContext, RequestEntropy, SDK_VERSION, SDK_VERSION_CODE, advance_ladon,
};
use hongguo_signer::ticket::SignedRequest;

use super::device::{rom_version, sdi_payload, signing_device};
use super::sdi::{Codec, Entropy, SdiToken, append_bytes, append_signed};
use super::{APP, MetaSec};
use crate::client::{ApiEnv, refuse_external_send};

const SDI_ORIGIN: &str = "https://mssdk.zijieapi.com";
const SDI_PATH: &str = "/sdi/get_token";
const PLAY_FLAGS: u64 = 0x0504;
const SDI_FLAGS: u64 = 2;
const COLD_COUNTER: i64 = 694_367;
const QUERY_FIELDS: &[&str] = &[
    "os",
    "ssmix",
    "_rticket",
    "cdid",
    "channel",
    "aid",
    "app_name",
    "version_code",
    "version_name",
    "manifest_version_code",
    "update_version_code",
    "resolution",
    "dpi",
    "device_type",
    "device_brand",
    "language",
    "os_api",
    "os_version",
    "ac",
    "host_abi",
    "dragon_device_type",
    "pv_player",
    "compliance_status",
    "need_personal_recommend",
    "player_so_load",
    "is_android_pad_screen",
    "rom_version",
    "iid",
    "device_id",
    "klink_egdi",
];
const SDI_PREFIX: &[&str] = &[
    "lc_id",
    "platform",
    "device_platform",
    "sdk_ver",
    "sdk_ver_code",
    "app_ver",
    "version_code",
    "aid",
    "sdkid",
    "subaid",
    "iid",
    "did",
    "bd_did",
    "client_type",
    "region_type",
    "mode",
    "full_update_version_code",
];

thread_local! {
    static LADON: Cell<u32> = const { Cell::new(0) };
}

struct WireRequest<'a> {
    origin: &'a str,
    path: &'a str,
    body: Vec<u8>,
    ticket: u64,
    sdk: bool,
}

pub(crate) async fn sign_playback(
    client: &reqwest::Client,
    origin: &str,
    path: &str,
    body: &[u8],
    headers: &[(String, String)],
    env: &ApiEnv,
) -> AppResult<Option<SignedRequest>> {
    if !path.starts_with("/novel/player/")
        && !matches!(
            path,
            crate::history::READ_HISTORY_UPDATE_PATH | crate::history::READ_PROGRESS_UPLOAD_PATH
        )
    {
        return Ok(None);
    }
    let Some(metasec) = MetaSec::configured()? else {
        return Ok(None);
    };
    refuse_external_send(origin)?;
    let (profile_key, session) = metasec.session(&env.device)?;
    let initial = session
        .initial
        .get_or_try_init(|| async {
            if let Some(app) = APP.get() {
                let store = app.sessions.clone();
                let key = profile_key.clone();
                let cached = tokio::task::spawn_blocking(move || store.load(&key))
                    .await
                    .map_err(|_| AppError::Internal("MetaSec session read task failed".into()))??;
                if let Some(cached) = cached
                    && matches!(cached.status, 200 | 202)
                    && !cached.value.is_empty()
                    && cached.value.len() <= 256
                {
                    log::debug!("MetaSec restored SDK token with status {}", cached.status);
                    return Ok(cached);
                }
            }
            exchange_and_save(metasec, client, &env.device, &profile_key, None).await
        })
        .await?;
    let token = if initial.status == 202 {
        &session
            .refresh
            .get_or_try_init(|| {
                exchange_and_save(
                    metasec,
                    client,
                    &env.device,
                    &profile_key,
                    Some(&initial.value),
                )
            })
            .await?
            .value
    } else {
        &initial.value
    };
    let request = WireRequest {
        origin,
        path,
        body: body.to_vec(),
        ticket: now_millis()?,
        sdk: false,
    };
    Ok(Some(sign(
        metasec,
        &env.device,
        request,
        headers.to_vec(),
        Some(token),
    )?))
}

async fn exchange_and_save(
    metasec: &MetaSec,
    client: &reqwest::Client,
    device: &DeviceProfile,
    profile_key: &str,
    token: Option<&str>,
) -> AppResult<SdiToken> {
    let issued = exchange(metasec, client, device, token).await?;
    if let Some(app) = APP.get() {
        let store = app.sessions.clone();
        let key = profile_key.to_owned();
        let session = issued.clone();
        tokio::task::spawn_blocking(move || store.save(&key, &session))
            .await
            .map_err(|_| AppError::Internal("MetaSec session write task failed".into()))??;
    }
    log::debug!(
        "MetaSec SDI exchange returned token status {}",
        issued.status
    );
    Ok(issued)
}

async fn exchange(
    metasec: &MetaSec,
    client: &reqwest::Client,
    device: &DeviceProfile,
    token: Option<&str>,
) -> AppResult<SdiToken> {
    refuse_external_send(SDI_ORIGIN)?;
    let codec = Codec {
        common_key: metasec.signer.common_key(),
        inner_key: metasec.signer.image().sdi_inner_key(),
    };
    let ticket = now_millis()?;
    let request = WireRequest {
        origin: SDI_ORIGIN,
        path: SDI_PATH,
        ticket,
        sdk: true,
        body: codec.encode_request(
            &sdi_payload(device, metasec.signer.image(), token),
            ticket,
            Entropy {
                nonce: rand::random(),
                frame_head: rand::random(),
            },
        )?,
    };
    let headers = vec![
        ("content-type".into(), "application/octet-stream".into()),
        ("accept".into(), "*/*".into()),
        ("x-tt-request-tag".into(), "t=0;n=0".into()),
        ("x-ss-dp".into(), device.get("aid").into()),
        ("cookie".into(), anonymous_cookie(device)),
    ];
    let signed = sign(metasec, device, request, headers, token)?;
    let mut request = client.post(signed.url);
    for (name, value) in signed.headers {
        request = request.header(name, value);
    }
    let response = request
        .body(signed.body.expect("SDI body"))
        .send()
        .await
        .map_err(|error| AppError::Network(error.without_url().to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::Network(format!(
            "MetaSec SDI HTTP {}",
            response.status()
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| AppError::Network(error.without_url().to_string()))?;
    if bytes.is_empty() {
        return Err(AppError::EmptyResponse(
            "MetaSec SDI returned an empty body".into(),
        ));
    }
    codec.decode_response(&bytes)
}

fn sign(
    metasec: &MetaSec,
    device: &DeviceProfile,
    request: WireRequest<'_>,
    mut headers: Vec<(String, String)>,
    token: Option<&str>,
) -> AppResult<SignedRequest> {
    let query = query(device, request.ticket, request.sdk)?;
    let khronos = (request.ticket / 1000) as u32;
    let url = format!("{}{}?{query}", request.origin, request.path);
    refuse_external_send(&url)?;
    let state = SignatureState::new(metasec, device, request.ticket, token, request.sdk);
    let context = RequestContext {
        device_id: device.get("device_id"),
        version_name: device.get("version_name"),
        token,
        counters: &state.counters,
        environment: &state.environment,
        runtime_json: &state.runtime_json,
        flags: if request.sdk { SDI_FLAGS } else { PLAY_FLAGS },
        ladon: LADON.with(|ladon| {
            let next = advance_ladon(ladon.get(), rand::random::<u64>() & !15, khronos);
            ladon.set(next);
            next
        }),
    };
    let factors = metasec
        .signer
        .factors(
            &query,
            Some(&request.body),
            khronos,
            &context,
            &RequestEntropy::draw(),
        )
        .map_err(|error| AppError::Network(error.to_string()))?;
    headers.extend(factors);
    headers.extend([
        ("user-agent".into(), cronet_user_agent(device)),
        ("x-vc-bdturing-sdk-version".into(), "4.0.3.cn".into()),
        (
            "x-ss-stub".into(),
            hongguo_signer::md5_hex_upper(&request.body),
        ),
        (
            "x-tt-trace-id".into(),
            format!(
                "00-{:032x}-{:016x}-01",
                rand::random::<u128>(),
                rand::random::<u64>()
            ),
        ),
    ]);
    if !request.sdk {
        headers.extend([
            (
                "content-type".into(),
                "application/json; charset=utf-8".into(),
            ),
            (
                "accept".into(),
                "application/json; charset=utf-8,application/x-protobuf".into(),
            ),
            ("x-xs-from-web".into(), "0".into()),
            ("sdk-version".into(), "2".into()),
            ("passport-sdk-version".into(), "5051452".into()),
            ("x-ss-req-ticket".into(), request.ticket.to_string()),
            (
                "x-reading-request".into(),
                format!("{}-{}", request.ticket, rand::random::<u32>()),
            ),
        ]);
    }
    Ok(SignedRequest {
        url,
        headers,
        body: Some(request.body),
    })
}

fn query(device: &DeviceProfile, ticket: u64, sdk: bool) -> AppResult<String> {
    let mut url = reqwest::Url::parse("https://query.invalid")
        .map_err(|_| AppError::Internal("MetaSec query URL".into()))?;
    let ticket = ticket.to_string();
    let sdk_version_code = SDK_VERSION_CODE.to_string();
    {
        let mut pairs = url.query_pairs_mut();
        let prefix = if sdk {
            SDI_PREFIX
        } else {
            &["device_platform"]
        };
        for key in prefix {
            let value = match *key {
                "lc_id" => LICENSE_CHANNEL,
                "platform" => "android",
                "sdk_ver" => SDK_VERSION,
                "sdk_ver_code" => &sdk_version_code,
                "app_ver" => device.get("version_name"),
                "did" => device.get("device_id"),
                "client_type" => "inhouse",
                "region_type" => "ml",
                "mode" => "2",
                "full_update_version_code" => device.get("update_version_code"),
                _ => device.get(key),
            };
            pairs.append_pair(key, value);
        }
        for key in QUERY_FIELDS {
            if sdk && matches!(*key, "aid" | "iid" | "version_code") {
                continue;
            }
            let value = match *key {
                "_rticket" => &ticket,
                "need_personal_recommend" => "1",
                "rom_version" => rom_version(device),
                _ => device.get(key),
            };
            pairs.append_pair(key, value);
        }
    }
    let query = url
        .query()
        .ok_or_else(|| AppError::Internal("MetaSec query missing".into()))?;
    if sdk {
        Ok(query.to_owned())
    } else {
        Ok(query.replacen("&host_abi=", "&&host_abi=", 1))
    }
}

fn cronet_user_agent(device: &DeviceProfile) -> String {
    let fingerprint = fingerprint_for_cdid(device.get("cdid"));
    format!(
        "com.phoenix.read/{} (Linux; U; Android {}; zh_CN; {}; Build/{}; Cronet/TTNetVersion:8d40f833 2026-03-03 QuicVersion:21ac1950 2025-11-18)",
        device.get("version_code"),
        fingerprint.os,
        fingerprint.display,
        fingerprint.build
    )
}

fn now_millis() -> AppResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .map_err(|_| AppError::Internal("invalid system clock for MetaSec".into()))
}

struct SignatureState {
    counters: Vec<u8>,
    environment: Vec<u8>,
    runtime_json: String,
}

impl SignatureState {
    fn new(
        metasec: &MetaSec,
        device: &DeviceProfile,
        ticket: u64,
        token: Option<&str>,
        sdk: bool,
    ) -> Self {
        static SIGNED: AtomicU64 = AtomicU64::new(0);
        let count = SIGNED.fetch_add(1, Ordering::Relaxed) + 1;
        let mut counters = Vec::new();
        for (field, value) in [
            (1, count as i64),
            (2, COLD_COUNTER),
            (3, COLD_COUNTER),
            (5, COLD_COUNTER),
        ] {
            append_signed(&mut counters, field, value);
        }
        let mut environment = Vec::new();
        append_signed(&mut environment, 1, if sdk { 1 } else { count as i64 });
        append_bytes(&mut environment, 6, b"v04.10.03.04-bugfix");
        append_signed(&mut environment, 7, i64::from(std::process::id()));
        append_bytes(&mut environment, 8, metasec.environment_label.as_bytes());
        append_bytes(&mut environment, 12, &signing_device(device));
        append_bytes(&mut environment, 13, &[]);
        append_bytes(&mut environment, 14, device.get("version_name").as_bytes());
        append_signed(&mut environment, 15, 2114);
        append_signed(&mut environment, 19, ticket as i64);
        let runtime_json = serde_json::json!({
            "cmr": 16_777_216, "cmr2": 16_777_216, "vpn": 0,
            "kd": 0, "dyn": "", "do": 0, "tk": token.is_some()
        })
        .to_string();
        Self {
            counters,
            environment,
            runtime_json,
        }
    }
}

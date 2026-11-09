//! 抓取流请求级快照：A=隔离同进程注册后 PLAY_OK；B=二次进程预热后同 vid。
//! 用法：
//!   HG_DUMP_REQ_DIR=... HG_PHASE=A HG_TARGET_VID=... cargo run -p hongguo-service --example dump_play_req_compare -- <data_dir>
//!   HG_DUMP_REQ_DIR=... HG_PHASE=B HG_DEVICE_JSON=... HG_TARGET_VID=... cargo run ... -- ignored
use hongguo_api::client::ApiEnv;
use hongguo_api::play_url::{fetch_play_url, register_content_type};
use hongguo_api::stream_pick::DecoderCapability;
use hongguo_service::app_state::AppStateInner;
use hongguo_service::device::{BootstrapMeta, register_and_swap, write_meta};
use hongguo_signer::device::{DeviceProfile, anonymous_cookie, anonymous_profile, fresh_cdid};
use hongguo_store::Store;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let phase = std::env::var("HG_PHASE").unwrap_or_else(|_| "A".into());
    match phase.as_str() {
        "A" => phase_a().await,
        "B" => phase_b().await,
        other => panic!("HG_PHASE must be A or B, got {other}"),
    }
}

async fn phase_a() {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: dump_play_req_compare <data_dir>");
    let target = std::env::var("HG_TARGET_VID").unwrap_or_default();
    let out_dir = std::env::var("HG_DUMP_REQ_DIR").expect("HG_DUMP_REQ_DIR");
    std::fs::create_dir_all(&dir).expect("mkdir");
    unsafe {
        std::env::set_var("HONGGUO_DATA_DIR", &dir);
        std::env::remove_var("HONGGUO_OFFLINE_QA");
        std::env::remove_var("HONGGUO_REUSE_DEVICE_JSON");
        std::env::set_var("HG_DUMP_REQ_TAG", "A-same-process");
    }
    let db = std::path::Path::new(&dir).join("hongguo.db");
    let store = Store::open(&db).expect("open");
    let anon = anonymous_profile(&fresh_cdid());
    store.save_device_profile(&anon).expect("seed");
    write_meta(&store, &BootstrapMeta::default());
    let state = Arc::new(AppStateInner::with_device(store, anon));

    println!("PHASE=A registering in {dir} …");
    let reg = register_and_swap(&state, "dump-req-compare-A").await;
    println!(
        "register ok={} iid={} err={:?}",
        reg.ok, reg.iid, reg.error
    );
    if !reg.ok {
        std::process::exit(2);
    }

    let env = state.api_env();
    let cookie = env.cookie.clone().unwrap_or_default();
    println!(
        "cookie_has_ttreq={} cookie_has_iid={} x_tt_token={}",
        cookie.contains("ttreq="),
        cookie.contains("install_id=") || cookie.contains("iid="),
        env.x_tt_token.as_ref().map(|t| t.len()).unwrap_or(0)
    );

    // Prefer explicit target vid, else feed picks.
    let mut tries: Vec<(i64, String, String)> = Vec::new();
    if !target.is_empty() {
        tries.push((1004, target.clone(), "HG_TARGET_VID".into()));
    }
    match hongguo_api::recommend::fetch_recommend_feed("16", "", 0, &[], &env).await {
        Ok(page) => {
            for it in page.items.iter().filter(|i| !i.vid.is_empty()).take(5) {
                if tries.iter().any(|(_, v, _)| v == &it.vid) {
                    continue;
                }
                tries.push((it.content_type, it.vid.clone(), it.title.clone()));
            }
        }
        Err(e) => println!("FEED_ERR {e}"),
    }

    let mut play_ok_vid = None;
    for (ct, vid, title) in &tries {
        register_content_type(vid, *ct);
        unsafe {
            std::env::set_var(
                "HG_DUMP_REQ_TAG",
                format!("A-same-process-try-{}", &vid[..8.min(vid.len())]),
            );
        }
        print!("A try ct={ct} vid={vid} title={title} … ");
        match fetch_play_url(vid, None, DecoderCapability::full(), &env).await {
            Ok(info) => {
                println!("PLAY_OK def={} codec={}", info.definition, info.codec);
                play_ok_vid = Some((vid.clone(), *ct));
                break;
            }
            Err(e) => println!("ERR {e}"),
        }
    }

    let profile = state.device();
    let profile_path = std::path::Path::new(&out_dir).join("device-after-A.json");
    std::fs::write(
        &profile_path,
        serde_json::to_string_pretty(&profile).unwrap(),
    )
    .unwrap();
    if let Some((vid, ct)) = play_ok_vid {
        let meta = serde_json::json!({
            "phase": "A",
            "play_ok_vid": vid,
            "ct": ct,
            "iid": profile.get("iid"),
            "cdid": profile.get("cdid"),
            "device_json": profile_path.file_name().and_then(|s| s.to_str()),
        });
        std::fs::write(
            std::path::Path::new(&out_dir).join("A-PLAY_OK-meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();
        println!("A_DONE play_ok_vid={vid} device={}", profile_path.display());
    } else {
        println!("A_FAIL no PLAY_OK");
        std::process::exit(4);
    }
}

async fn phase_b() {
    let device_path = std::env::var("HG_DEVICE_JSON").expect("HG_DEVICE_JSON");
    let vid = std::env::var("HG_TARGET_VID").expect("HG_TARGET_VID");
    let ct: i64 = std::env::var("HG_PROBE_CT")
        .unwrap_or_else(|_| "1004".into())
        .parse()
        .unwrap();
    let warm = std::env::var("HG_WARMUP").unwrap_or_else(|_| "1".into()) == "1";
    unsafe {
        std::env::set_var("HG_DUMP_REQ_TAG", "B-second-process-pre");
    }
    let device: DeviceProfile =
        serde_json::from_str(&std::fs::read_to_string(&device_path).unwrap()).unwrap();
    let env = ApiEnv {
        proxy: Default::default(),
        cookie: Some(anonymous_cookie(&device)),
        x_tt_token: None,
        device: device.clone(),
    };
    println!(
        "PHASE=B iid={} cdid={} warm={warm} vid={vid} cookie_ttreq={}",
        device.get("iid"),
        device.get("cdid"),
        env.cookie.as_deref().unwrap_or("").contains("ttreq=")
    );
    if warm {
        let n = hongguo_api::warmup::run(&env).await;
        println!("warmup_ok={n}");
    }
    register_content_type(&vid, ct);
    unsafe {
        std::env::set_var("HG_DUMP_REQ_TAG", "B-second-process-after-warmup");
    }
    match fetch_play_url(&vid, None, DecoderCapability::full(), &env).await {
        Ok(info) => {
            println!(
                "B_PLAY_OK def={} codec={} url60={}",
                info.definition,
                info.codec,
                &info.url[..60.min(info.url.len())]
            );
        }
        Err(e) => {
            println!("B_EMPTY_OR_ERR {e}");
            std::process::exit(5);
        }
    }
}

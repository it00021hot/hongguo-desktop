//! 同进程：注册 PLAY_OK → JSON roundtrip 新 ApiEnv → 再取同 vid。区分「落盘字段」vs「进程边界」。
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
    let dir = std::env::args().nth(1).expect("data_dir");
    let vid = std::env::var("HG_TARGET_VID").unwrap_or_else(|_| "7686010748832582681".into());
    std::fs::create_dir_all(&dir).unwrap();
    unsafe {
        std::env::set_var("HONGGUO_DATA_DIR", &dir);
        std::env::remove_var("HONGGUO_OFFLINE_QA");
    }
    let store = Store::open(&std::path::Path::new(&dir).join("hongguo.db")).unwrap();
    let anon = anonymous_profile(&fresh_cdid());
    store.save_device_profile(&anon).unwrap();
    write_meta(&store, &BootstrapMeta::default());
    let state = Arc::new(AppStateInner::with_device(store, anon));
    let reg = register_and_swap(&state, "sameproc-roundtrip").await;
    println!("register ok={} iid={}", reg.ok, reg.iid);
    if !reg.ok { std::process::exit(2); }
    let env1 = state.api_env();
    register_content_type(&vid, 1004);
    match fetch_play_url(&vid, None, DecoderCapability::full(), &env1).await {
        Ok(i) => println!("SAMEPROC_DIRECT PLAY_OK def={}", i.definition),
        Err(e) => { println!("SAMEPROC_DIRECT ERR {e}"); std::process::exit(3); }
    }
    let json = serde_json::to_string(&state.device()).unwrap();
    let reloaded: DeviceProfile = serde_json::from_str(&json).unwrap();
    let env2 = ApiEnv {
        proxy: Default::default(),
        cookie: Some(anonymous_cookie(&reloaded)),
        x_tt_token: None,
        device: reloaded.clone(),
    };
    println!(
        "roundtrip iid_same={} ttreq_same={} cookie_ttreq={}",
        reloaded.get("iid") == state.device().get("iid"),
        reloaded.ttreq() == state.device().ttreq(),
        env2.cookie.as_deref().unwrap_or("").contains("ttreq=")
    );
    // clear play cache so we actually hit network again
    hongguo_api::play_url::clear_batch_cache();
    match fetch_play_url(&vid, None, DecoderCapability::full(), &env2).await {
        Ok(i) => println!("SAMEPROC_ROUNDTRIP PLAY_OK def={}", i.definition),
        Err(e) => println!("SAMEPROC_ROUNDTRIP ERR {e}"),
    }
}

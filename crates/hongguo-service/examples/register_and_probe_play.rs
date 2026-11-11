//! QA one-shot：隔离库注册一次并在同一进程内验证取流。
use hongguo_api::play_url::{fetch_play_url, register_content_type};
use hongguo_api::stream_pick::DecoderCapability;
use hongguo_service::app_state::AppStateInner;
use hongguo_service::device::{BootstrapMeta, read_meta, register_and_swap, write_meta};
use hongguo_signer::device::{anonymous_profile, fresh_cdid};
use hongguo_store::Store;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: register_and_probe_play <data_dir>");
    std::fs::create_dir_all(&dir).expect("mkdir");
    unsafe {
        std::env::set_var("HONGGUO_DATA_DIR", &dir);
        std::env::remove_var("HONGGUO_OFFLINE_QA");
        std::env::remove_var("HONGGUO_REUSE_DEVICE_JSON");
    }
    let db = std::path::Path::new(&dir).join("hongguo.db");
    let store = Store::open(&db).expect("open");
    let anon = anonymous_profile(&fresh_cdid());
    store.save_device_profile(&anon).expect("seed");
    write_meta(&store, &BootstrapMeta::default());
    let state = Arc::new(AppStateInner::with_device(store, anon));

    println!("registering in {dir} …");
    let reg = register_and_swap(&state, "sign1004-isolated-probe").await;
    println!(
        "register ok={} iid={} kind={} err={:?}",
        reg.ok, reg.iid, reg.kind, reg.error
    );
    if !reg.ok {
        std::process::exit(2);
    }

    let env = state.api_env();
    println!(
        "cookie_has_ttreq={} cookie_has_iid={}",
        env.cookie.as_deref().unwrap_or("").contains("ttreq="),
        env.cookie.as_deref().unwrap_or("").contains("install_id=")
            || env.cookie.as_deref().unwrap_or("").contains("iid=")
    );

    let page = match hongguo_api::recommend::fetch_recommend_feed("16", "", 0, &[], &env).await {
        Ok(p) => p,
        Err(e) => {
            println!("FEED_ERR {e}");
            std::process::exit(3);
        }
    };
    let picks: Vec<_> = page
        .items
        .iter()
        .filter(|i| !i.vid.is_empty())
        .take(5)
        .collect();
    println!("feed picks={}", picks.len());
    let mut any_ok = false;
    for it in picks {
        register_content_type(&it.vid, it.content_type);
        print!(
            "try ct={} vid={} title={} … ",
            it.content_type, it.vid, it.title
        );
        match fetch_play_url(&it.vid, None, DecoderCapability::full(), &env).await {
            Ok(info) => {
                println!("PLAY_OK def={} codec={}", info.definition, info.codec);
                any_ok = true;
                break;
            }
            Err(e) => println!("ERR {e}"),
        }
    }
    let meta = read_meta(&state.store);
    println!(
        "done any_ok={any_ok} source={} regime={}",
        meta.source, meta.register_regime
    );
    if !any_ok {
        std::process::exit(4);
    }
}

//! 同进程：PLAY_OK vid1 后清缓存，再取从未打过的 vid2 / 再取 vid1。
use hongguo_api::play_url::{clear_batch_cache, fetch_play_url, register_content_type};
use hongguo_api::stream_pick::DecoderCapability;
use hongguo_service::app_state::AppStateInner;
use hongguo_service::device::{BootstrapMeta, register_and_swap, write_meta};
use hongguo_signer::device::{anonymous_profile, fresh_cdid};
use hongguo_store::Store;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let dir = std::env::args().nth(1).expect("data_dir");
    let vid1 = std::env::var("HG_TARGET_VID").unwrap_or_else(|_| "7686010748832582681".into());
    std::fs::create_dir_all(&dir).unwrap();
    unsafe {
        std::env::set_var("HONGGUO_DATA_DIR", &dir);
        std::env::remove_var("HONGGUO_OFFLINE_QA");
    }
    let store = Store::open(std::path::Path::new(&dir).join("hongguo.db")).unwrap();
    let anon = anonymous_profile(&fresh_cdid());
    store.save_device_profile(&anon).unwrap();
    write_meta(&store, &BootstrapMeta::default());
    let state = Arc::new(AppStateInner::with_device(store, anon));
    let reg = register_and_swap(&state, "second-vid").await;
    println!("register ok={} iid={}", reg.ok, reg.iid);
    if !reg.ok { std::process::exit(2); }
    let env = state.api_env();
    register_content_type(&vid1, 1004);
    match fetch_play_url(&vid1, None, DecoderCapability::full(), &env).await {
        Ok(i) => println!("VID1_FIRST PLAY_OK def={}", i.definition),
        Err(e) => { println!("VID1_FIRST ERR {e}"); std::process::exit(3); }
    }
    clear_batch_cache();
    match fetch_play_url(&vid1, None, DecoderCapability::full(), &env).await {
        Ok(i) => println!("VID1_SECOND PLAY_OK def={}", i.definition),
        Err(e) => println!("VID1_SECOND ERR {e}"),
    }
    let page = hongguo_api::recommend::fetch_recommend_feed("16", "", 0, &[], &env)
        .await
        .expect("feed");
    let mut n_ok = 0;
    for it in page.items.iter().filter(|i| !i.vid.is_empty() && i.vid != vid1).take(3) {
        register_content_type(&it.vid, it.content_type);
        print!("VID2_TRY ct={} vid={} title={} … ", it.content_type, it.vid, it.title);
        match fetch_play_url(&it.vid, None, DecoderCapability::full(), &env).await {
            Ok(info) => { println!("PLAY_OK def={}", info.definition); n_ok += 1; break; }
            Err(e) => println!("ERR {e}"),
        }
    }
    println!("other_vid_ok={n_ok}");
}

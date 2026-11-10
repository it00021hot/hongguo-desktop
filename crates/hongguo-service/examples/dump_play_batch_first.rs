use hongguo_api::play_url::{fetch_play_batch, fetch_play_url, register_content_type};
use hongguo_api::stream_pick::DecoderCapability;
use hongguo_service::app_state::AppStateInner;
use hongguo_service::device::{BootstrapMeta, register_and_swap, write_meta};
use hongguo_signer::device::{anonymous_profile, fresh_cdid};
use hongguo_store::Store;
use std::collections::BTreeMap;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let dir = std::env::args().nth(1).expect("data_dir");
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
    let reg = register_and_swap(&state, "batch-first").await;
    println!("register ok={} iid={}", reg.ok, reg.iid);
    let env = state.api_env();
    let page = hongguo_api::recommend::fetch_recommend_feed("16", "", 0, &[], &env).await.unwrap();
    let picks: Vec<_> = page.items.iter().filter(|i| !i.vid.is_empty()).take(3).collect();
    let mut map = BTreeMap::new();
    let mut group = Vec::new();
    for it in &picks {
        register_content_type(&it.vid, it.content_type);
        group.push(it.vid.clone());
        println!("pick ct={} vid={} title={}", it.content_type, it.vid, it.title);
    }
    map.insert("1004".into(), group.clone());
    let n = fetch_play_batch(&map, DecoderCapability::full(), &env).await.unwrap();
    println!("batch_hits={n}/{}", group.len());
    for vid in &group {
        match fetch_play_url(vid, None, DecoderCapability::full(), &env).await {
            Ok(i) => println!("FROM_CACHE_OR_NET {vid} OK def={}", i.definition),
            Err(e) => println!("FROM_CACHE_OR_NET {vid} ERR {e}"),
        }
    }
}

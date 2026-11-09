//! QA one-shot：把本机已验证可取流的可信 DeviceProfile 装进指定 hongguo.db。
//! 不发起注册；不清理 register_block_unknown。
use hongguo_common::utils::time::now_ms;
use hongguo_service::device::{read_meta, write_meta};
use hongguo_signer::device::DeviceProfile;
use hongguo_store::Store;

fn main() {
    let db_path = std::env::args().nth(1).expect("db path");
    let profile_path = std::env::args().nth(2).expect("profile json");
    let raw = std::fs::read_to_string(&profile_path).expect("read profile");
    let profile: DeviceProfile = serde_json::from_str(&raw).expect("parse profile");
    assert!(!profile.get("iid").is_empty(), "iid empty");
    assert!(profile.ttreq().is_some(), "ttreq missing");

    let store = Store::open(std::path::Path::new(&db_path)).expect("open store");
    let old_iid = store
        .device_profile()
        .ok()
        .flatten()
        .map(|p| p.get("iid").to_string())
        .unwrap_or_default();
    let _ = store.backup_device_profile();
    store.save_device_profile(&profile).expect("save profile");

    let mut meta = read_meta(&store);
    meta.source = "registered".into();
    meta.previous_iid = Some(old_iid).filter(|s| !s.is_empty() && s != profile.get("iid"));
    meta.register_regime = hongguo_api::register::REGISTER_REGIME.into();
    meta.last_register_error = None;
    meta.last_register_attempt_ms = Some(now_ms());
    meta.play_shipped = true;
    write_meta(&store, &meta);

    let check = store.device_profile().expect("reread").expect("present");
    let meta2 = read_meta(&store);
    println!(
        "installed iid={} prev={:?} regime={} play_shipped={} source={}",
        check.get("iid"),
        meta2.previous_iid,
        meta2.register_regime,
        meta2.play_shipped,
        meta2.source
    );
}

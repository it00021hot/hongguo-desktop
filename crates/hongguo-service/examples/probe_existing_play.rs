//! Probe an existing device with the current playback flow.
use hongguo_api::client::ApiEnv;
use hongguo_api::play_url::{fetch_play_url, register_content_type};
use hongguo_api::stream_pick::DecoderCapability;
use hongguo_signer::device::{DeviceProfile, anonymous_cookie};

#[tokio::main]
async fn main() {
    let path = std::env::args().nth(1).expect("device json");
    let device: DeviceProfile =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let env = ApiEnv {
        proxy: Default::default(),
        cookie: Some(anonymous_cookie(&device)),
        x_tt_token: None,
        device: device.clone(),
    };
    let page = hongguo_api::recommend::fetch_recommend_feed("16", "", 0, &[], &env)
        .await
        .expect("feed");
    let mut ok = 0usize;
    for it in page.items.iter().filter(|i| !i.vid.is_empty()).take(3) {
        register_content_type(&it.vid, it.content_type);
        print!("try {} ct={} vid={} … ", it.title, it.content_type, it.vid);
        match fetch_play_url(&it.vid, None, DecoderCapability::full(), &env).await {
            Ok(info) => {
                println!("PLAY_OK def={} codec={}", info.definition, info.codec);
                ok += 1;
                break;
            }
            Err(e) => println!("ERR {e}"),
        }
    }
    println!("ok_count={ok}");
    if ok == 0 {
        std::process::exit(4);
    }
}

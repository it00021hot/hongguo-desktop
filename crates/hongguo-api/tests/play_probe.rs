//! 取流请求探针（手工跑，#[ignore] 门控）：真播链（批量 player 优先 →
//! 单集回落）直调 fetch_play_url，验证选流出直链。
//! `cargo test -p hongguo-api --test play_probe -- --ignored --nocapture`

use hongguo_api::client::ApiEnv;
use hongguo_api::play_url::fetch_play_url;
use hongguo_api::stream_pick::DecoderCapability;

/// 抓包对账用：HG_PROBE_PROXY=1 时走本机 mitmdump（127.0.0.1:8080）。
fn probe_env() -> ApiEnv {
    let mut env = ApiEnv::anonymous(Default::default());
    if std::env::var("HG_PROBE_PROXY").is_ok() {
        env.proxy = hongguo_common::model::ProxyConfig {
            mode: hongguo_common::model::settings::ProxyMode::Manual,
            url: "http://127.0.0.1:8080".into(),
        };
    }
    env
}

#[tokio::test]
#[ignore = "手工探针：打真实接口"]
async fn probe_play_url() {
    let env = probe_env();
    // 拉两个 tab 的 feed：入口 vid 的 content_type 进注册表（真播批量
    // 分组 key 的来源），并直接用 feed 返回的 vid 验证「批量 player 优先」
    // 链（feed 内容按时间桶轮换，写死 vid 会脱 rotation 失去注册）
    let mut picks: Vec<(String, String)> = Vec::new();
    for tab in ["36", "39"] {
        if let Ok(page) = hongguo_api::recommend::fetch_recommend_feed(tab, "", 0, &[], &env).await
            && let Some(it) = page.items.iter().find(|i| !i.vid.is_empty())
        {
            picks.push((format!("tab{tab} ct={}", it.content_type), it.vid.clone()));
        }
    }
    for (name, vid) in &picks {
        match fetch_play_url(vid, None, DecoderCapability::Full, &env).await {
            Ok(info) => println!(
                "{name} OK 定义={} codec={} url前60={}",
                info.definition,
                info.codec,
                &info.url[..60.min(info.url.len())]
            ),
            Err(e) => println!("{name} ERR {e}"),
        }
    }
    // 未注册类型的 vid：应直接走单集回落，不猜 key
    match fetch_play_url("7680041581440683070", None, DecoderCapability::Full, &env).await {
        Ok(info) => println!(
            "未注册类型 OK（单集回落）定义={} url前60={}",
            info.definition,
            &info.url[..60.min(info.url.len())]
        ),
        Err(e) => println!("未注册类型 ERR {e}"),
    }
}

#[tokio::test]
#[ignore = "手工探针：打真实接口"]
async fn probe_play_batch() {
    use std::collections::BTreeMap;
    let env = hongguo_api::client::ApiEnv::anonymous(Default::default());
    // 官方实流样本：6 vid（全 1004 漫剧），加一组真人剧（ct=1）
    let map = BTreeMap::from([
        (
            "1004".to_string(),
            "7698309376561400856,7694644152008461336,7683090053475617816,7680669690372312089,7678678025646394430,7685684183628254270"
                .split(',')
                .map(str::to_string)
                .collect::<Vec<_>>(),
        ),
        ("1".to_string(), vec!["7687873056332598296".to_string()]),
    ]);
    match hongguo_api::play_url::fetch_play_batch(&map, DecoderCapability::Full, &env).await {
        Ok(n) => println!(
            "BATCH OK 暂存 {n}/{}",
            map.values().map(Vec::len).sum::<usize>()
        ),
        Err(e) => println!("BATCH ERR {e}"),
    }
}

/// 官方批量端点考证探针：
/// - model/player + dr_scene=player 发货（起播批量形态）；
/// - mixed_video_id_map 的 key = 条目 content_type 原样串，错配 → 空响应；
/// - detail/preload 逗号串批量一次回多剧。
#[tokio::test]
#[ignore = "手工探针：打真实接口"]
async fn probe_official_preload() {
    use std::collections::BTreeMap;
    let env = hongguo_api::client::ApiEnv::anonymous(Default::default());
    // 急诊摸鱼王 ep1（feed ct=1004）；三大反骨仔 ep1（tab39 feed ct=1）
    let anime_vid = "7680669690372312089".to_string();
    let human_vid = "7687873056332598296".to_string();

    for (name, map) in [
        (
            "A ct=1004放1004 + ct=1放1",
            BTreeMap::from([
                ("1004".to_string(), vec![anime_vid.clone()]),
                ("1".to_string(), vec![human_vid.clone()]),
            ]),
        ),
        (
            "B key错配 ct=1004放1001",
            BTreeMap::from([("1001".to_string(), vec![anime_vid.clone()])]),
        ),
    ] {
        let payload = hongguo_api::params::player_model_payload(&map);
        let body = serde_json::to_vec(&payload).unwrap();
        match hongguo_api::client::api_call_full(
            hongguo_api::client::LQ_API_ORIGIN,
            hongguo_api::params::PLAYER_MODEL_PATH,
            Some(body),
            &[],
            &env,
        )
        .await
        {
            Ok(bytes) => {
                let got: Vec<&String> = map.values().flatten().collect();
                println!(
                    "{name}: {} 字节，命中 vids {:?}",
                    bytes.len(),
                    got.iter()
                        .map(|v| serde_json::from_slice::<serde_json::Value>(&bytes)
                            .ok()
                            .and_then(|j| j
                                .pointer(&format!("/data/{v}/video_model"))
                                .is_some()
                                .then_some(v.as_str()))
                            .unwrap_or("?"))
                        .collect::<Vec<_>>()
                );
            }
            Err(e) => println!("{name}: ERR {e}"),
        }
    }

    // 单集端点官方切集形态（ct=剧类型动态 + source:4）：真人剧 ct=1 验证
    let payload = hongguo_api::params::single_model_payload(&human_vid, 1);
    let body = serde_json::to_vec(&payload).unwrap();
    match hongguo_api::client::api_call_full(
        hongguo_api::client::LQ_API_ORIGIN,
        hongguo_api::params::SINGLE_MODEL_PATH,
        Some(body),
        &[],
        &env,
    )
    .await
    {
        Ok(bytes) => {
            let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let has_model = v.pointer("/data/video_model").is_some();
            println!(
                "SINGLE ct=1: {} 字节，video_model 在场={has_model}",
                bytes.len()
            );
        }
        Err(e) => println!("SINGLE ct=1 ERR {e}"),
    }

    // detail/preload 逗号串批量（官方：series_id="id1,id2"）
    let series_batch = "7680564528135818302,7683070896516369470".to_string();
    let payload = hongguo_api::params::detail_payload(&series_batch);
    let body = serde_json::to_vec(&payload).unwrap();
    match hongguo_api::client::api_call_full(
        hongguo_api::client::LQ_API_ORIGIN,
        hongguo_api::params::DETAIL_PATH,
        Some(body),
        &[],
        &env,
    )
    .await
    {
        Ok(bytes) => {
            let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let keys = v
                .pointer("/data")
                .map(|d| d.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()));
            println!("DETAIL 批量: data keys = {keys:?}");
        }
        Err(e) => println!("DETAIL 批量 ERR {e}"),
    }
}

#[tokio::test]
#[ignore = "手工探针：打真实接口"]
async fn probe_feed_vids() {
    let env = hongguo_api::client::ApiEnv::anonymous(Default::default());
    for tab in ["36", "39"] {
        match hongguo_api::recommend::fetch_recommend_feed(tab, "", 0, &[], &env).await {
            Ok(page) => {
                println!(
                    "[feed:{tab}] ct 集合 = {:?}",
                    page.items
                        .iter()
                        .map(|i| i.content_type)
                        .collect::<Vec<_>>()
                );
                for it in page.items.iter().take(3) {
                    println!(
                        "  ct={:<5} vid={:<20} title={}",
                        it.content_type, it.vid, it.title
                    );
                }
            }
            Err(e) => println!("[feed:{tab}] ERR {e}"),
        }
    }
}

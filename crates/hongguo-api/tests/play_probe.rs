//! v5 取流请求探针（手工跑，需本地 MetaSec v5 材料）。
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
        match fetch_play_url(vid, None, DecoderCapability::full(), &env).await {
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
    match fetch_play_url("7680041581440683070", None, DecoderCapability::full(), &env).await {
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
    match hongguo_api::play_url::fetch_play_scene(
        &map,
        hongguo_api::params::PlayerModelScene::Player,
        DecoderCapability::full(),
        &env,
    )
    .await
    {
        Ok(n) => println!(
            "BATCH OK 暂存 {n}/{}",
            map.values().map(Vec::len).sum::<usize>()
        ),
        Err(e) => println!("BATCH ERR {e}"),
    }
}

/// 发货维度矩阵探针（临时取证）：一台新设备 × 5 个不同 vid × 2 轮取流，
/// 回答「失败挂在哪一维」——同 vid 重试会不会翻盘（请求级抖动）、不同
/// vid 有没有稳定差异（per-vid 门控）。
#[tokio::test]
#[ignore = "手工探针：打真实接口"]
async fn probe_delivery_matrix() {
    let env = hongguo_api::client::ApiEnv::anonymous(Default::default());
    let reg = hongguo_api::register::register_device(&env)
        .await
        .expect("注册失败");
    let mut device =
        hongguo_signer::device::DeviceProfile::fresh_register_profile(&reg.cdid, &reg.openudid);
    device.set("device_id", &reg.device_id);
    device.set("iid", &reg.install_id);
    device.set_ttreq(Some(reg.ttreq.clone()));
    let cookie = hongguo_signer::device::anonymous_cookie(&device);
    let dev_env = hongguo_api::client::ApiEnv {
        proxy: Default::default(),
        device,
        cookie: Some(cookie),
        x_tt_token: None,
    };
    println!("[matrix] iid={}", reg.install_id);

    let page = hongguo_api::recommend::fetch_recommend_feed("36", "", 0, &[], &dev_env)
        .await
        .expect("feed 失败");
    let vids: Vec<(String, i64)> = page
        .items
        .iter()
        .filter(|i| !i.vid.is_empty())
        .take(5)
        .map(|i| (i.vid.clone(), i.content_type))
        .collect();
    for round in 1..=2 {
        for (vid, ct) in &vids {
            hongguo_api::play_url::register_content_type(vid, *ct);
            let r = match hongguo_api::play_url::fetch_play_url(
                vid,
                None,
                hongguo_api::stream_pick::DecoderCapability::full(),
                &dev_env,
            )
            .await
            {
                Ok(_) => "OK".to_string(),
                Err(e) => format!("ERR({e})").chars().take(48).collect(),
            };
            println!("[matrix] round{round} ct={ct} vid={vid} -> {r}");
        }
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

#[tokio::test]
#[ignore = "手工探针：用 HONGGUO_REUSE_DEVICE_JSON 对账取流空响应"]
async fn probe_sign_1004_with_reuse_device() {
    use hongguo_signer::device::{DeviceProfile, anonymous_cookie};
    let path = std::env::var("HG_DEVICE_JSON").expect("HG_DEVICE_JSON");
    let raw = std::fs::read_to_string(&path).expect("read device json");
    let device: DeviceProfile = serde_json::from_str(&raw).expect("parse device");
    assert!(!device.get("iid").is_empty(), "iid");
    assert!(device.ttreq().is_some(), "ttreq required");
    let cookie = anonymous_cookie(&device);
    let env = ApiEnv {
        proxy: Default::default(),
        cookie: Some(cookie),
        x_tt_token: None,
        device: device.clone(),
    };
    println!(
        "device iid={} brand={} type={} ttreq_len={}",
        device.get("iid"),
        device.get("device_brand"),
        device.get("device_type"),
        device.ttreq().map(|t| t.len()).unwrap_or(0)
    );

    // 先拉推荐拿活 vid，再打 brush 失败集
    let mut cases: Vec<(String, i64, String)> = Vec::new();
    if let Ok(page) = hongguo_api::recommend::fetch_recommend_feed("16", "", 0, &[], &env).await {
        for it in page.items.iter().filter(|i| !i.vid.is_empty()).take(2) {
            cases.push((
                format!("feed:{}", it.title),
                it.content_type,
                it.vid.clone(),
            ));
        }
    }
    // brush 空响应目标 + 同会话曾命中的 vid
    cases.push(("brush-fail".into(), 1004, "7697465811157404697".into()));
    cases.push((
        "brush-hit-after-swap".into(),
        1004,
        "7701967150029753368".into(),
    ));

    for (name, ct, vid) in &cases {
        hongguo_api::play_url::register_content_type(vid, *ct);
        match fetch_play_url(vid, None, DecoderCapability::full(), &env).await {
            Ok(info) => println!(
                "OK {name} vid={vid} def={} codec={} url60={}",
                info.definition,
                info.codec,
                &info.url[..60.min(info.url.len())]
            ),
            Err(e) => println!("ERR {name} vid={vid} {e}"),
        }
    }
}

#[tokio::test]
#[ignore = "手工探针：单 vid"]
async fn probe_single_vid_device() {
    use hongguo_signer::device::{DeviceProfile, anonymous_cookie};
    let path = std::env::var("HG_DEVICE_JSON").expect("HG_DEVICE_JSON");
    let vid = std::env::var("HG_PROBE_VID").expect("HG_PROBE_VID");
    let ct: i64 = std::env::var("HG_PROBE_CT")
        .unwrap_or_else(|_| "1004".into())
        .parse()
        .unwrap();
    let device: DeviceProfile =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let env = ApiEnv {
        proxy: Default::default(),
        cookie: Some(anonymous_cookie(&device)),
        x_tt_token: None,
        device: device.clone(),
    };
    hongguo_api::play_url::register_content_type(&vid, ct);
    println!("iid={} ct={} vid={}", device.get("iid"), ct, vid);
    match fetch_play_url(&vid, None, DecoderCapability::full(), &env).await {
        Ok(info) => println!(
            "OK def={} codec={} url60={}",
            info.definition,
            info.codec,
            &info.url[..60.min(info.url.len())]
        ),
        Err(e) => println!("ERR {e}"),
    }
}

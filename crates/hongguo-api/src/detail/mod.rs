//! 分集列表：调接口 + 解析。分集在响应里的位置固定：
//! `data[series_id].video_data.video_list[]`（不要「递归找第一个数组」——
//! video_data 下还有 celebrities 等数组，猜出来必错且不报错）。

mod model;
mod parse;

use serde_json::Value;

use hongguo_common::{AppError, AppResult};

pub use model::{EpisodeList, RelatedSeries, SeriesCreator, SeriesMeta};
// 保持公共路径 `detail::RelatedItem` 可用；公开再导出后 rustc 不再按 unused_imports 处理
pub use model::RelatedItem;
pub use parse::{parse_episodes, parse_episodes_batch, parse_series_meta};

use parse::parse_related_series;

/// 相关作品端点（详情页「相关推荐」懒加载，lq 域轻签名）：
/// 响应 `data[]` 是 cell 列表，`cell_name:"相关作品"` 的 cell 里 `video_data[]` 带 `tag_info.text` 角标。
pub const PLAN_PATH: &str = "/reading/bookapi/plan/v";

/// 拉一部剧的相关作品·系列。plan/v 首次调用下发冷响应（guess 子格稀少甚至空，连打逐次填充），
/// 故 guess 空时递增退避重试（至多 4 调）取首个非空；全空退最后一次响应。
pub async fn fetch_related_series(
    series_id: &str,
    env: &super::client::ApiEnv,
) -> AppResult<RelatedSeries> {
    let biz_query: Vec<(String, String)> = [
        ("book_id", series_id),
        ("bookstore_tab", "0"),
        ("bookstore_tab_type", "0"),
        ("current_chapter_num", "0"),
        ("from", "detail_page_more_related"),
        ("is_horizontal_screen", "false"),
        ("limit", "0"),
        ("need_personal_recommend", "1"),
        ("offset", "0"),
        ("post_id", "0"),
        ("scene", "10"),
        ("total_chapter_num", "0"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    let mut last = RelatedSeries::default();
    /// 相关推荐的重试次数与递增退避（400/700/1000ms）。
    const PLAN_RETRY_ATTEMPTS: u8 = 4;
    const PLAN_RETRY_BASE_MS: u64 = 400;
    const PLAN_RETRY_STEP_MS: u64 = 300;
    for attempt in 0..PLAN_RETRY_ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(
                PLAN_RETRY_BASE_MS + PLAN_RETRY_STEP_MS * (attempt - 1) as u64,
            ))
            .await;
        }
        let bytes = crate::client::api_call_reading(
            crate::danmaku::LQ_API_ORIGIN,
            PLAN_PATH,
            None,
            &biz_query,
            env,
        )
        .await?;

        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
        let related = parse_related_series(&value)?;
        if !related.guess.is_empty() {
            return Ok(related);
        }
        last = related;
    }
    Ok(last)
}

/// 取一部剧的分集列表。
pub async fn fetch_episode_list(
    series_id: &str,
    env: &super::client::ApiEnv,
) -> AppResult<EpisodeList> {
    let payload = serde_json::to_vec(&crate::params::detail_payload(series_id))
        .map_err(|e| AppError::Signer(e.to_string()))?;

    // sinfoninec 域 + gzip body（sinfonlineb 旧形态已整体回空）
    let bytes = crate::client::api_call_reading(
        crate::client::SINFONLINEC_API_ORIGIN,
        crate::params::DETAIL_PATH,
        Some(payload),
        &[],
        env,
    )
    .await?;

    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
    register_episode_types(&value);
    parse_episodes(&value)
}

/// 批量拉多部剧的分集档案（官方预取形态）：一次 `multi_video_detail/preload`，
/// `series_id` 是**逗号串**（官方 6 部一发）；单部残缺由解析层跳过。
pub async fn fetch_episode_lists_batch(
    series_ids: &[String],
    env: &super::client::ApiEnv,
) -> AppResult<Vec<EpisodeList>> {
    let joined = series_ids.join(",");
    let payload = serde_json::to_vec(&crate::params::detail_payload(&joined))
        .map_err(|e| AppError::Signer(e.to_string()))?;
    let bytes = crate::client::api_call_reading(
        crate::client::SINFONLINEC_API_ORIGIN,
        crate::params::DETAIL_PATH,
        Some(payload),
        &[],
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析批量详情响应失败: {e}")))?;
    register_episode_types(&value);
    parse_episodes_batch(&value)
}

/// 档案响应里的 vid → content_type 进注册表（批量取流分组 key 的来源之一）。
fn register_episode_types(response: &Value) {
    let Some(data) = response.get("data").and_then(Value::as_object) else {
        return;
    };
    for node in data.values() {
        let Some(video_data) = node.get("video_data") else {
            continue;
        };
        let ct = video_data
            .get("content_type")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if ct == 0 {
            continue;
        }
        for ep in video_data
            .get("video_list")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            if let Some(vid) = ep.get("vid").and_then(Value::as_str) {
                crate::play_url::register_content_type(vid, ct);
            }
        }
    }
}

/// 详情页头部元信息端点：`POST /novel/player/video_detail/v1/`。
/// 追剧数/播放量/季徽/题材标签/备案号都在这里——preload 分集接口不带这些。
pub const VIDEO_DETAIL_PATH: &str = "/novel/player/video_detail/v1/";

/// 拉详情页头部元信息。失败交调用方降级（头部缺这几行不影响主功能）。
pub async fn fetch_series_meta(
    series_id: &str,
    env: &super::client::ApiEnv,
) -> AppResult<SeriesMeta> {
    // body 照官方（audit 纪律）：screen_width_px 是字符串形态
    let body = serde_json::to_vec(&crate::params::video_detail_payload(series_id))
        .map_err(|e| AppError::Signer(e.to_string()))?;

    let bytes = super::client::api_call_reading(
        super::client::SINFONLINEC_API_ORIGIN,
        VIDEO_DETAIL_PATH,
        Some(body),
        &[],
        env,
    )
    .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析详情元信息失败: {e}")))?;
    parse_series_meta(&value, series_id)
}

#[cfg(test)]
mod probe {
    use super::*;

    /// 详情响应里的公开计数盘点（信息流 comment_count 恒 0，右栏计数需真实来源）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_detail_counts() {
        let env = crate::client::ApiEnv::anonymous(hongguo_common::model::ProxyConfig::default());
        let payload =
            serde_json::to_vec(&crate::params::detail_payload("7690883800057777177")).unwrap();
        let bytes = crate::client::api_call(crate::params::DETAIL_PATH, Some(payload), &env)
            .await
            .expect("detail");
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let series = &v["data"]["7690883800057777177"];
        let first_ep = &series["video_data"]["video_list"][0];
        let keys: Vec<String> = first_ep
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        let count_keys: Vec<&String> = keys
            .iter()
            .filter(|k| k.contains("count") || k.contains("cnt"))
            .collect();
        println!("[detail-counts] 首集计数类键: {count_keys:?}");
        for k in count_keys {
            println!("  {k} = {}", first_ep[k]);
        }
        for ptr in [
            "/video_data/followed_cnt",
            "/video_data/video_detail/followed_cnt",
            "/followed_cnt",
        ] {
            if let Some(v) = series.pointer(ptr) {
                println!("  followed_cnt 路径: {ptr} = {v}");
            }
        }
        let s = serde_json::to_string(series).unwrap_or_default();
        for key in ["digged_count", "followed_cnt", "comment_count", "play_cnt"] {
            let n = s.matches(&format!("\"{key}\"")).count();
            println!("  响应中 \"{key}\" 出现 {n} 次");
        }
    }
}

#[cfg(test)]
mod probe2 {
    use super::*;

    /// 活体探测：plan/v 真实 cell 结构 + 剧评评分字段（PROBE_SERIES_ID 可指定）。
    #[ignore = "直连真实接口的探测用例"]
    #[tokio::test]
    async fn probe_fengshen_live() {
        let proxy = hongguo_common::model::ProxyConfig::default();
        let env = match std::env::var("PROBE_DEVICE_FILE") {
            Ok(path) => {
                let json = std::fs::read_to_string(&path).expect("读设备档案");
                let device = serde_json::from_str(&json).expect("解析设备档案");
                crate::client::ApiEnv {
                    proxy,
                    device,
                    cookie: None,
                    x_tt_token: None,
                }
            }
            Err(_) => crate::client::ApiEnv::anonymous(proxy),
        };
        let sid = std::env::var("PROBE_SERIES_ID").unwrap_or_else(|_| "7685637575473630270".into());

        let rel = fetch_related_series(&sid, &env).await.expect("plan");
        println!("[plan] works={} guess={}", rel.works.len(), rel.guess.len());
        for w in rel.works.iter().chain(rel.guess.iter()).take(6) {
            println!(
                "[plan]   {} | {} | tag={} score={} ep={} play={}",
                w.series_id, w.title, w.tag, w.score, w.episode_cnt, w.play_cnt
            );
        }

        match super::super::danmaku::fetch_series_comments_page(&sid, "", &env).await {
            Ok(page) => {
                println!(
                    "[reviews] total={} score={:?} score_cnt={} tags={:?}",
                    page.page.total, page.score, page.score_cnt, page.tags
                );
            }
            Err(e) => println!("[reviews] ERR: {e}"),
        }

        match fetch_series_meta(&sid, &env).await {
            Ok(m) => println!(
                "[meta] title={} followed={} play={} score? season={} tags={:?} record={}",
                m.title, m.followed_cnt, m.play_cnt, m.season, m.tags, m.record_number
            ),
            Err(e) => println!("[meta] ERR: {e}"),
        }

        let payload = serde_json::to_vec(&serde_json::json!({
            "biz_param": {
                "caller_scene": "single_col", "detail_page_version": 1,
                "disable_digg_stat": false, "disable_video_relate_book": false,
                "from_video_id": "", "need_all_video_definition": false,
                "need_mp4_align": false, "screen_width_px": "1078", "source": 4,
                "use_os_player": false, "use_server_dns": false, "video_id_type": 1,
            },
            "series_id": sid,
        }))
        .unwrap();
        let vd_bytes = crate::client::api_call(VIDEO_DETAIL_PATH, Some(payload), &env)
            .await
            .expect("video_detail");
        let vd_raw = String::from_utf8_lossy(&vd_bytes).to_string();
        for key in ["\"score\"", "rating", "digg_cnt", "comment_count"] {
            let mut from = 0;
            for _ in 0..2 {
                let Some(pos) = vd_raw[from..].find(key) else {
                    break;
                };
                let at = from + pos;
                let lo = at.saturating_sub(40);
                let hi = (at + 100).min(vd_raw.len());
                println!(
                    "[vdetail] {key} @ {at}: ...{}",
                    vd_raw[lo..hi].replace(char::is_whitespace, " ")
                );
                from = at + 1;
            }
        }

        let biz_query: Vec<(String, String)> = [
            ("book_id", sid.as_str()),
            ("bookstore_tab", "0"),
            ("bookstore_tab_type", "0"),
            ("current_chapter_num", "0"),
            ("from", "detail_page_more_related"),
            ("is_horizontal_screen", "false"),
            ("limit", "0"),
            ("need_personal_recommend", "1"),
            ("offset", "0"),
            ("post_id", "0"),
            ("scene", "10"),
            ("total_chapter_num", "0"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let bytes = crate::client::api_call_reading(
            super::super::danmaku::LQ_API_ORIGIN,
            PLAN_PATH,
            None,
            &biz_query,
            &env,
        )
        .await
        .expect("plan raw");
        if let Ok(dir) = std::env::var("PROBE_DUMP") {
            let path = std::path::Path::new(&dir).join("plan-raw.json");
            std::fs::write(&path, &bytes).expect("写 plan-raw.json");
            println!("[plan raw] dumped -> {}", path.display());
        }
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        for cell in v
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = cell.get("cell_name").and_then(Value::as_str).unwrap_or("?");
            let n = cell
                .get("video_data")
                .and_then(Value::as_array)
                .map(|a| a.len())
                .unwrap_or(0);
            println!("[plan raw] cell={name} items={n}");
            if let Some(obj) = cell.as_object() {
                for (k, val) in obj {
                    if k == "video_data" || k == "cell_name" {
                        continue;
                    }
                    if let Some(arr) = val.as_array() {
                        println!("[plan raw]   cell={name} arr {k} len={}", arr.len());
                        if let Some(first) = arr.first() {
                            let keys: Vec<_> = first
                                .as_object()
                                .map(|o| o.keys().cloned().collect())
                                .unwrap_or_default();
                            println!("[plan raw]     first keys: {keys:?}");
                            println!(
                                "[plan raw]     first: {}",
                                serde_json::to_string(first).unwrap_or_default()
                            );
                        }
                    }
                }
            }
        }
        let raw = String::from_utf8_lossy(&bytes).to_string();
        for key in ["guess_mvs", "guess"] {
            let mut from = 0;
            for _ in 0..3 {
                let Some(pos) = raw[from..].find(key) else {
                    break;
                };
                let at = from + pos;
                let lo = at.saturating_sub(60);
                let hi = (at + 120).min(raw.len());
                println!(
                    "[plan raw] {key} @ {at}: ...{}...",
                    raw[lo..hi].replace(char::is_whitespace, " ")
                );
                from = at + 1;
            }
        }
    }

    #[ignore = "直连真实接口的探测用例"]
    #[tokio::test]
    async fn probe_detail_related_series() {
        let env = crate::client::ApiEnv::anonymous(hongguo_common::model::ProxyConfig::default());
        let payload =
            serde_json::to_vec(&crate::params::detail_payload("7689382439004671038")).unwrap();
        let bytes = crate::client::api_call(crate::params::DETAIL_PATH, Some(payload), &env)
            .await
            .expect("detail");
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let node = &v["data"]["7689382439004671038"];
        let mut keys: Vec<_> = node.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        println!("[rel] data.<sid> keys: {keys:?}");
        let vd = &node["video_data"];
        let mut vkeys: Vec<_> = vd
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        vkeys.sort();
        println!("[rel] video_data keys: {vkeys:?}");
        let s = serde_json::to_string(&v).unwrap();
        for key in [
            "relation",
            "relate_series",
            "series_list",
            "season",
            "同IP",
            "相关",
            "recommend",
        ] {
            let n = s.matches(key).count();
            if n > 0 {
                println!("  [{key}] x{n}");
            }
        }
    }
}

#[cfg(test)]
mod probe_related {
    use super::*;
    /// 相关作品·系列真连探测（多季剧才有多条）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_related_series_live() {
        let env = crate::client::ApiEnv::anonymous(hongguo_common::model::ProxyConfig::default());
        let rel = fetch_related_series("7687961503718198334", &env)
            .await
            .expect("相关作品");
        println!(
            "[related] works {} 条, guess {} 条",
            rel.works.len(),
            rel.guess.len()
        );
        for w in rel.works.iter().take(6) {
            println!(
                "  [{}] {} score={} {}集 play={}",
                w.tag, w.title, w.score, w.episode_cnt, w.play_cnt
            );
        }
        assert!(!rel.works.is_empty(), "多季剧必有相关作品");
    }
}

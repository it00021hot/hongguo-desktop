//! 分集列表：调接口 + 解析。
//!
//! 分集在响应里的位置是固定的：`data[series_id].video_data.video_list[]`。
//! 不要用「递归找第一个数组」的办法——`video_data` 下面还有 `celebrities`、
//! `abstract_tags` 等一堆数组，猜出来的必然是错的，而且不会报错。

mod model;
mod parse;

use serde_json::Value;

use crate::error::{AppError, AppResult};

pub use model::{EpisodeList, RelatedSeries, SeriesMeta};
// 保持既有公共路径 `detail::RelatedItem` 不变（crate 内暂无直接引用者）
#[expect(
    unused_imports,
    reason = "RelatedItem 只被 RelatedSeries 聚合引用，re-export 为兼容旧路径保留"
)]
pub use model::RelatedItem;
pub use parse::{parse_episodes, parse_series_meta};

use parse::parse_related_series;

/// 相关作品（同系列各季 + 同 IP 作品）。
///
/// 端点抓包实证（2026-10-07 hgplayer 1.1.5 详情页「相关推荐」tab 懒加载）：
/// `GET /reading/bookapi/plan/v?book_id=<series_id>&from=detail_page_more_related
/// &scene=10&...`（reading 族轻签名头，lq 域）。响应 `data[]` 是 cell 列表：
/// `cell_name:"相关作品"` 的 cell 里 `video_data[]` 每项带 `tag_info.text`
/// 角标（`第1季`/`同IP`/…）。
pub const PLAN_PATH: &str = "/reading/bookapi/plan/v";

/// 拉一部剧的相关作品·系列。
///
/// 服务端对 plan/v 的**首次调用**下发冷响应——猜你喜欢 cell 还在但
/// `cell_data` 里的短剧子格稀少甚至为空，连打会逐次填充（2026-10-08
/// 实测同进程 4 连打 guess 1→6→8→9）。客户端每次进详情页只打一次、
/// 前端还带 10 分钟缓存，冷响应会原样上屏成「猜你喜欢消失」。
/// 对策：guess 空时递增退避重试（至多 4 调），取首个非空结果；works
/// 各次稳定不受影响，全空就退最后一次的响应。
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
    for attempt in 0..4u8 {
        if attempt > 0 {
            // 递增退避：400/700/1000ms。实测冷填充通常第二次就有数据，
            // 连冷到第四次极罕见（间隔拉长给服务端填充留时间）。
            tokio::time::sleep(std::time::Duration::from_millis(
                400 + 300 * (attempt - 1) as u64,
            ))
            .await;
        }
        let bytes = crate::domain::api::client::api_call_reading(
            crate::domain::api::danmaku::LQ_API_ORIGIN,
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
    let payload = serde_json::to_vec(&crate::domain::api::params::detail_payload(series_id))
        .map_err(|e| AppError::Signer(e.to_string()))?;

    let bytes = crate::domain::api::client::api_call(
        crate::domain::api::params::DETAIL_PATH,
        Some(payload),
        env,
    )
    .await?;

    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析响应失败: {e}")))?;
    parse_episodes(&value)
}

// ---------------------------------------------------------------- 剧集元信息

/// 详情页头部元信息端点（2026-10-07 抓 hgplayer 详情页锁定）：
/// `POST /novel/player/video_detail/v1/`，body `{"biz_param":{…},"series_id"}`。
/// 追剧数/播放量/季徽/题材标签/备案号都在这里——preload 分集接口不带这些。
pub const VIDEO_DETAIL_PATH: &str = "/novel/player/video_detail/v1/";

/// 拉详情页头部元信息。失败交调用方降级（头部缺这几行不影响主功能）。
pub async fn fetch_series_meta(
    series_id: &str,
    env: &super::client::ApiEnv,
) -> AppResult<SeriesMeta> {
    // body 照抄抓包（audit 纪律）：screen_width_px 是字符串形态
    let body = serde_json::to_vec(&serde_json::json!({
        "biz_param": {
            "caller_scene": "single_col",
            "detail_page_version": 1,
            "disable_digg_stat": false,
            "disable_video_relate_book": false,
            "from_video_id": "",
            "need_all_video_definition": false,
            "need_mp4_align": false,
            "screen_width_px": "1078",
            "source": 4,
            "use_os_player": false,
            "use_server_dns": false,
            "video_id_type": 1,
        },
        "series_id": series_id,
    }))
    .map_err(|e| AppError::Signer(e.to_string()))?;

    let bytes = super::client::api_call(VIDEO_DETAIL_PATH, Some(body), env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析详情元信息失败: {e}")))?;
    parse_series_meta(&value, series_id)
}

#[cfg(test)]
mod probe {
    use super::*;

    /// 详情响应里的公开计数盘点（信息流 landpage 的 comment_count 恒 0、
    /// 无点赞/收藏数——hgplayer 右栏计数需要一个真实来源）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_detail_counts() {
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        let payload = serde_json::to_vec(&crate::domain::api::params::detail_payload(
            "7690883800057777177",
        ))
        .unwrap();
        let bytes = crate::domain::api::client::api_call(
            crate::domain::api::params::DETAIL_PATH,
            Some(payload),
            &env,
        )
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
        // followed_cnt 的路径定位（候选指针逐个试）
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

    /// 活体探测：plan/v 的真实 cell 结构 + 剧评响应 extra 的评分字段。
    /// 用法：`PROBE_SERIES_ID=<id> cargo test probe_live -- --ignored --nocapture`
    /// （默认疯神镇妖官）。probe_detail_related_series 是原有探测，见下。
    #[ignore = "直连真实接口的探测用例"]
    #[tokio::test]
    async fn probe_fengshen_live() {
        let proxy = crate::domain::model::ProxyConfig::default();
        // PROBE_DEVICE_FILE 给真实设备档案（默认静态兜底档案）：
        // 评分/猜你喜欢疑随设备信任度下发，匿名静态档案拿到的是空。
        let env = match std::env::var("PROBE_DEVICE_FILE") {
            Ok(path) => {
                let json = std::fs::read_to_string(&path).expect("读设备档案");
                let device = serde_json::from_str(&json).expect("解析设备档案");
                crate::domain::api::client::ApiEnv {
                    proxy,
                    device,
                    cookie: None,
                    x_tt_token: None,
                }
            }
            Err(_) => crate::domain::api::client::ApiEnv::anonymous(proxy),
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

        // video_detail 原始响应里搜评分键（头部 8.0分 的可能来源）
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
        let vd_bytes = crate::domain::api::client::api_call(VIDEO_DETAIL_PATH, Some(payload), &env)
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

        // 原始 plan 响应的 cell 名与条数（不经解析，防解析器吞内容）
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
        let bytes = crate::domain::api::client::api_call_reading(
            super::super::danmaku::LQ_API_ORIGIN,
            PLAN_PATH,
            None,
            &biz_query,
            &env,
        )
        .await
        .expect("plan raw");
        // PROBE_DUMP_SET 时把原始响应落盘，供离线分析（服务端结构常变，
        // 反复编译探测太慢）。
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
            // cell 里的其它数组字段（hgplayer 的 guess_mvs tag 暗示猜你喜欢
            // 可能不走 video_data 而走专用字段）
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
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
        let payload = serde_json::to_vec(&crate::domain::api::params::detail_payload(
            "7689382439004671038",
        ))
        .unwrap();
        let bytes = crate::domain::api::client::api_call(
            crate::domain::api::params::DETAIL_PATH,
            Some(payload),
            &env,
        )
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
    /// 相关作品·系列真连探测（2026-10-07 端点实装验证；多季剧才有多条）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_related_series_live() {
        let env = crate::domain::api::client::ApiEnv::anonymous(
            crate::domain::model::ProxyConfig::default(),
        );
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

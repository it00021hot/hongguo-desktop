//! 云端观看历史（`read_history/list/v`，2026-10-05 抓包）。
//!
//! 官方 App 侧栏「历史」的数据源：账号维度的播放记录（含在官方客户端
//! 看的），比本地播放档案全。query 只要 `limit/offset/query_soft_deleted`，
//! 登录态必备（匿名回空表）。
//!
//! 注意字段形态：`vid`/`book_id` 在响应里是 JSON 数字，统一转成字符串
//! 承载，避免 js 侧精度失真。

mod model;

pub use model::{WatchHistoryItem, WatchHistoryPage};

use serde_json::Value;

use super::client::{ApiEnv, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use crate::error::{AppError, AppResult};
use crate::utils::json::{check_code, str_field};
use crate::utils::time::now_ms;

pub const READ_HISTORY_LIST_PATH: &str = "/reading/bookapi/read_history/list/v";
const READ_HISTORY_UPDATE_PATH: &str = "/reading/bookapi/read_history/update/v";
const READ_PROGRESS_UPLOAD_PATH: &str = "/reading/bookapi/read_progress/upload/v";

/// 拉一页云端观看历史。
pub async fn fetch_watch_history(offset: i64, env: &ApiEnv) -> AppResult<WatchHistoryPage> {
    let q: Vec<(String, String)> = [
        // 2026-10-05 抓包逐字段对齐（captures/flows-20261005.jsonl）。
        // book_type=2 是**短剧过滤**的关键：缺了它会落进无名字的阅读历史
        // 子集（audit 的 DEVICE_QS 过滤把这几个字段当设备参数藏了，
        // 排查时必须 dump 抓包原文）。
        ("book_type", "2"),
        ("full_field", "false"),
        ("is_first_load", "true"),
        ("last_min_read_timestamp_ms", "0"),
        // limit=0 = 全量（官方形态）
        ("limit", "0"),
        ("offset", offset.to_string().as_str()),
        ("query_soft_deleted", "false"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let bytes = api_call_reading(LQ_API_ORIGIN, READ_HISTORY_LIST_PATH, None, &q, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析观看历史失败: {e}")))?;
    check_code(&value)?;
    parse_history(value.get("data"))
}

fn parse_history(data: Option<&Value>) -> AppResult<WatchHistoryPage> {
    let data = data.ok_or_else(|| AppError::Media("响应缺少 data".into()))?;
    let mut items = Vec::new();
    for raw in data
        .get("data_list")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        // vid/book_id 在响应里是 JSON 数字，精度外的会失真，统一走字符串化
        let series_id = raw
            .get("book_id_str")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| raw.get("book_id").map(num_to_string).unwrap_or_default());
        if series_id.is_empty() {
            continue;
        }
        items.push(WatchHistoryItem {
            series_id,
            title: str_field(raw, "book_name"),
            cover: str_field(raw, "thumb_url"),
            vid_index: raw
                .get("vid_index")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            vid: raw
                .get("vid_str")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| raw.get("vid").map(num_to_string).unwrap_or_default()),
            position_ms: raw
                .get("current_play_position")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            duration_ms: raw
                .get("duration")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            episode_cnt: raw
                .get("episode_cnt")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            updated_at_ms: raw
                .get("read_timestamp_ms")
                .and_then(Value::as_i64)
                .unwrap_or_default(),
        });
    }
    Ok(WatchHistoryPage {
        items,
        has_more: data
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        next_offset: data.get("next_offset").and_then(Value::as_i64).unwrap_or(0),
        total: data.get("total").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// JSON 数字 → 字符串（u64 精度无损；浮点兜底去尾零）。
fn num_to_string(v: &Value) -> String {
    match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// 批量删除云端观看历史（2026-10-11 逆向 hgplayer 1.1.8 delete.go 逐字段锁定）：
/// 复用 `read_history/update`，每条带 `is_delete=true + use_soft_delete=true`，
/// 其余进度字段全零（官方删除样本形态）。响应 `update_fail_datas` 非空 =
/// 部分失败，报错给上层。
pub async fn delete_watch_history(
    items: &[(String, String, i64)],
    env: &ApiEnv,
) -> AppResult<()> {
    if items.is_empty() {
        return Ok(());
    }
    let now = now_ms();
    let update_datas: Vec<Value> = items
        .iter()
        .map(|(series_id, vid, vid_index)| {
            serde_json::json!({
                "book_id": str_or_num(series_id),
                "book_type": 2,
                "chapter_index": 0,
                "current_play_position": 0,
                "digged_count": 0,
                "duration": 0,
                "episode_cnt": 0,
                "is_delete": true,
                "is_interactive_game": false,
                "is_listen_mode": false,
                "is_multi_season": 0,
                "meet_guide_comment_tag": false,
                "origin_novel_book_id": 0,
                "player_accumulate_total_time": 0,
                "read_timestamp_ms": 0,
                "recent_reads": 0,
                "retain_video_play_time": 0,
                "season_index": 0,
                "series_play_cnt": 0,
                "tone_id": 0,
                "update_timestamp_ms": now,
                "use_soft_delete": true,
                "user_digg": false,
                "user_playlet_comment_flag": false,
                "vid": str_or_num(vid),
                "vid_index": vid_index,
            })
        })
        .collect();
    let body = serde_json::json!({ "update_datas": update_datas });
    let raw = serde_json::to_vec(&body)
        .map_err(|e| AppError::Media(format!("构造历史删除请求失败: {e}")))?;
    let bytes = api_call_reading(LQ_API_ORIGIN, READ_HISTORY_UPDATE_PATH, Some(raw), &[], env)
        .await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析历史删除响应失败: {e}")))?;
    check_code(&value)?;
    let fail_datas = value
        .pointer("/data/update_fail_datas")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if fail_datas > 0 {
        return Err(AppError::Media(format!("部分历史删除失败（{fail_datas} 条）")));
    }
    Ok(())
}

/// 观看进度上报（2026-10-06 抓 hgplayer 1.1.5 双接口逐字段锁定）：
/// 官方客户端播片时同时打 `read_history/update` 与 `read_progress/upload`，
/// 云端「历史」页的写入端。hgplayer 约每分钟一次 + 切集时触发。
///
/// 两接口字段形态刻意不同（照抓包原样）：update 里 book_id/vid 是 JSON
/// **数字**，upload 里 book_id/item_id 是**字符串**；duration/retain 等
/// hgplayer 传 0 的字段保持 0。`player_accumulate_total_time` 官方样本里
/// 等于当时进度，这里同用 position_ms 承载（我们未单独累计观看时长）。
pub async fn report_watch_progress(
    series_id: &str,
    vid: &str,
    vid_index: i64,
    position_ms: i64,
    env: &ApiEnv,
) -> AppResult<()> {
    let now = now_ms();
    let book_num = str_or_num(series_id);
    let vid_num = str_or_num(vid);
    let update_body = serde_json::json!({
        "update_datas": [{
            "book_id": book_num,
            "book_type": 2,
            "chapter_index": 0,
            "current_play_position": position_ms,
            "digged_count": 0,
            "duration": 0,
            "episode_cnt": 0,
            "is_delete": false,
            "is_interactive_game": false,
            "is_listen_mode": false,
            "is_multi_season": 0,
            "meet_guide_comment_tag": false,
            "origin_novel_book_id": 0,
            "player_accumulate_total_time": position_ms,
            "read_timestamp_ms": now,
            "recent_reads": 0,
            "retain_video_play_time": 0,
            "season_index": 0,
            "series_play_cnt": 0,
            "tone_id": 0,
            "update_timestamp_ms": now,
            "use_soft_delete": false,
            "user_digg": false,
            "user_playlet_comment_flag": false,
            "vid": vid_num,
            "vid_index": vid_index,
        }],
    });
    let upload_body = serde_json::json!({
        "books": [{
            "book_id": series_id,
            "book_type": 2,
            "channel_id": 0,
            "check_timestamp": false,
            "cur_channel_id": 0,
            "current_play_time": position_ms,
            "is_listen_mode": false,
            "is_local_book": false,
            "item_id": vid,
            "listen_and_read": false,
            "page_index": 0,
            "page_progress_rate": 0,
            "paragraph_offset": 0,
            "player_cumulative_total_duration": position_ms,
            "progress_type": 0,
            "read_timestamp_ms": now,
            "tone_id": 0,
            "vid_index": vid_index,
        }],
    });

    let update_raw = serde_json::to_vec(&update_body)
        .map_err(|e| AppError::Media(format!("构造历史上报失败: {e}")))?;
    let bytes = api_call_reading(
        LQ_API_ORIGIN,
        READ_HISTORY_UPDATE_PATH,
        Some(update_raw),
        &[],
        env,
    )
    .await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析历史上报响应失败: {e}")))?;
    check_code(&v)?;

    let upload_raw = serde_json::to_vec(&upload_body)
        .map_err(|e| AppError::Media(format!("构造进度上传失败: {e}")))?;
    let bytes = api_call_reading(
        LQ_API_ORIGIN,
        READ_PROGRESS_UPLOAD_PATH,
        Some(upload_raw),
        &[],
        env,
    )
    .await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析进度上传响应失败: {e}")))?;
    check_code(&v)?;
    Ok(())
}

/// 数字型 id 字段：可解析就发 JSON 数字（对齐抓包），否则原样字符串。
fn str_or_num(s: &str) -> Value {
    s.parse::<u64>()
        .map(|n| serde_json::json!(n))
        .unwrap_or_else(|_| serde_json::json!(s))
}

#[cfg(test)]
pub(crate) mod probe {
    use super::*;

    /// 实证云端历史真实返回（对号入座：同账号在 hgplayer 显示最近记录）。
    /// `PROBE_DEVICE` / `PROBE_COOKIE` 由真实库导出；`PROBE_COOKIE_HG` /
    /// `PROBE_TOKEN` 可选，用官方会话做对照。
    /// `cargo test probe_watch_history -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_watch_history() {
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let account_cookie = std::env::var("PROBE_COOKIE").unwrap_or_default();
        let hg_cookie = std::env::var("PROBE_COOKIE_HG")
            .ok()
            .filter(|c| !c.is_empty());
        let hg_token = std::env::var("PROBE_TOKEN").ok().filter(|t| !t.is_empty());

        let device: crate::signer::device::DeviceProfile =
            serde_json::from_str(&device_json).expect("device json");
        let mut device = device;
        crate::signer::device::align_app_version(&mut device);
        // 官方会话对照：抓包里 cookie 是逗号分隔（addon 形态），转分号
        let cookie_raw = hg_cookie.unwrap_or(account_cookie);
        let cookie = cookie_raw.replace(", ", "; ");
        let env = ApiEnv {
            proxy: crate::domain::model::ProxyConfig::default(),
            device,
            cookie: Some(cookie),
            x_tt_token: hg_token,
        };
        let page = fetch_watch_history(0, &env).await.expect("历史请求");
        println!("[history] total={} items={}", page.total, page.items.len());
        for it in page.items.iter().take(8) {
            println!(
                "  {:?} ep={} pos={}ms at={}",
                it.title,
                it.vid_index,
                it.position_ms,
                chrono_like(it.updated_at_ms)
            );
        }
    }

    /// unix 毫秒 → 本地可读时间（探测打印用，UTC+8 简化）。
    fn chrono_like(ms: i64) -> String {
        let dt = ms / 1000 + 8 * 3600;
        let days = dt.div_euclid(86_400);
        let rem = dt.rem_euclid(86_400);
        format!("day+{days} {:02}:{:02}", rem / 3600, (rem % 3600) / 60)
    }
}

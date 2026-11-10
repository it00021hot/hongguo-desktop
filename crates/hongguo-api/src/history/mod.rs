//! 云端观看历史（`read_history/list/v`）：账号维度播放记录（含在官方客户端
//! 看的），登录态必备（匿名回空表）。`vid`/`book_id` 响应里是 JSON 数字，
//! 统一转字符串承载，避免 js 侧精度失真。

mod model;

pub use model::{WatchHistoryItem, WatchHistoryPage};

use serde_json::Value;

use super::client::{ApiEnv, SINFONLINEA_API_ORIGIN, api_call_playback_v5, api_call_reading};
use super::danmaku::LQ_API_ORIGIN;
use hongguo_common::model::LocalHistoryItem;
use hongguo_common::utils::json::{check_code, str_field};
use hongguo_common::utils::time::now_ms;
use hongguo_common::{AppError, AppResult};

pub const READ_HISTORY_LIST_PATH: &str = "/reading/bookapi/read_history/list/v";
pub(crate) const READ_HISTORY_UPDATE_PATH: &str = "/reading/bookapi/read_history/update/v";
pub(crate) const READ_PROGRESS_UPLOAD_PATH: &str = "/reading/bookapi/read_progress/upload/v";

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestPlaybackReport {
    pub cumulative_ms: i64,
    pub observed_at_ms: i64,
    pub progress: Vec<GuestProgressItem>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestProgressItem {
    pub series_id: String,
    pub vid: String,
    pub position_ms: i64,
    pub cumulative_ms: i64,
    pub observed_at_ms: i64,
}

/// 拉一页云端观看历史。
pub async fn fetch_watch_history(offset: i64, env: &ApiEnv) -> AppResult<WatchHistoryPage> {
    let q: Vec<(String, String)> = [
        // book_type=2 是短剧过滤的关键：缺了会落进无名字的阅读历史子集
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
            // 线上 0 基 → +1 转回领域值（前端/本地存储 1 基）
            vid_index: raw
                .get("vid_index")
                .and_then(Value::as_i64)
                .unwrap_or_default()
                .max(0)
                + 1,
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

/// 历史删除沿用 0 基索引。
fn wire_vid_index(domain: i64) -> i64 {
    (domain - 1).max(0)
}

/// 批量删除云端观看历史：复用 `read_history/update`，每条带
/// `is_delete=true + use_soft_delete=true`；响应 `update_fail_datas` 非空 = 部分失败。
pub async fn delete_watch_history(items: &[(String, String, i64)], env: &ApiEnv) -> AppResult<()> {
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
                "vid_index": wire_vid_index(*vid_index),
            })
        })
        .collect();
    let body = serde_json::json!({ "update_datas": update_datas });
    let raw = serde_json::to_vec(&body)
        .map_err(|e| AppError::Media(format!("构造历史删除请求失败: {e}")))?;
    let bytes =
        api_call_reading(LQ_API_ORIGIN, READ_HISTORY_UPDATE_PATH, Some(raw), &[], env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析历史删除响应失败: {e}")))?;
    check_code(&value)?;
    let fail_datas = value
        .pointer("/data/update_fail_datas")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if fail_datas > 0 {
        return Err(AppError::Media(format!(
            "部分历史删除失败（{fail_datas} 条）"
        )));
    }
    Ok(())
}

/// 历史更新发送 1 基集号，进度上传通过 `item_id` 识别集。
pub async fn report_watch_progress(
    series_id: &str,
    vid: &str,
    vid_index: i64,
    position_ms: i64,
    env: &ApiEnv,
) -> AppResult<()> {
    let (update_body, upload_body) =
        watch_progress_bodies(series_id, vid, vid_index, position_ms, now_ms());
    let update_raw = serde_json::to_vec(&update_body)
        .map_err(|e| AppError::Media(format!("构造历史上报失败: {e}")))?;
    let bytes = api_call_reading(
        SINFONLINEA_API_ORIGIN,
        READ_HISTORY_UPDATE_PATH,
        Some(update_raw),
        &[],
        env,
    )
    .await?;
    let v: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析历史上报响应失败: {e}")))?;
    check_code(&v)?;
    check_update_fails(&v)?;

    let upload_raw = serde_json::to_vec(&upload_body)
        .map_err(|e| AppError::Media(format!("构造进度上传失败: {e}")))?;
    let bytes = api_call_reading(
        SINFONLINEA_API_ORIGIN,
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

/// 官方匿名播放在原设备会话下更新当前一集的历史。
pub async fn report_guest_watch_progress(
    item: &LocalHistoryItem,
    report: Option<&GuestPlaybackReport>,
    env: &ApiEnv,
) -> AppResult<()> {
    let body = history_update_body(
        &item.series_id,
        &item.vid,
        i64::from(item.vid_index),
        item.position_ms,
        report.map_or(item.position_ms, |sample| sample.cumulative_ms),
        report.map_or(item.updated_at_ms, |sample| sample.observed_at_ms),
        0,
    );
    let update = send_guest_report(READ_HISTORY_UPDATE_PATH, body, env).await;
    if update.is_ok() {
        log::debug!("[History] 匿名历史更新成功，集号 {}", item.vid_index);
    }
    let Some(report) = report.filter(|sample| !sample.progress.is_empty()) else {
        return update;
    };
    let books: Vec<_> = report.progress.iter().map(guest_progress_item).collect();
    let progress = send_guest_report(
        READ_PROGRESS_UPLOAD_PATH,
        serde_json::json!({"books": books}),
        env,
    )
    .await;
    if progress.is_ok() {
        log::debug!("[History] 匿名进度上传成功，共 {} 集", books.len());
    }
    update.and(progress)
}

async fn send_guest_report(path: &str, body: Value, env: &ApiEnv) -> AppResult<()> {
    let raw = serde_json::to_vec(&body)
        .map_err(|e| AppError::Media(format!("构造匿名播放上报失败: {e}")))?;
    let bytes = api_call_playback_v5(SINFONLINEA_API_ORIGIN, path, raw, env).await?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Media(format!("解析匿名播放上报响应失败: {e}")))?;
    check_code(&value)?;
    check_update_fails(&value)
}

fn watch_progress_bodies(
    series_id: &str,
    vid: &str,
    vid_index: i64,
    position_ms: i64,
    now: i64,
) -> (Value, Value) {
    let update_body = history_update_body(
        series_id,
        vid,
        vid_index,
        position_ms,
        position_ms,
        now,
        now,
    );
    let upload_body = serde_json::json!({"books": [progress_upload_item(series_id, vid, position_ms, position_ms, now)]});
    (update_body, upload_body)
}

fn history_update_body(
    series_id: &str,
    vid: &str,
    vid_index: i64,
    position_ms: i64,
    cumulative_ms: i64,
    now: i64,
    update_timestamp: i64,
) -> Value {
    serde_json::json!({
        "update_datas": [{
            "book_id": str_or_num(series_id),
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
            "player_accumulate_total_time": cumulative_ms,
            "read_timestamp_ms": now,
            "recent_reads": 0,
            "retain_video_play_time": 0,
            "season_index": 0,
            "series_play_cnt": 0,
            "tone_id": 0,
            "update_timestamp_ms": update_timestamp,
            "use_soft_delete": false,
            "user_digg": false,
            "user_playlet_comment_flag": false,
            "vid": str_or_num(vid),
            "vid_index": vid_index,
        }],
    })
}

fn guest_progress_item(sample: &GuestProgressItem) -> Value {
    progress_upload_item(
        &sample.series_id,
        &sample.vid,
        sample.position_ms,
        sample.cumulative_ms,
        sample.observed_at_ms,
    )
}

fn progress_upload_item(
    series_id: &str,
    vid: &str,
    position_ms: i64,
    cumulative_ms: i64,
    now: i64,
) -> Value {
    serde_json::json!({
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
            "player_cumulative_total_duration": cumulative_ms,
            "progress_type": 0,
            "read_timestamp_ms": now,
            "tone_id": 0,
            "vid_index": 0,
    })
}

/// 数字型 id 字段：可解析就发 JSON 数字（对齐官方），否则原样字符串。
fn str_or_num(s: &str) -> Value {
    s.parse::<u64>()
        .map(|n| serde_json::json!(n))
        .unwrap_or_else(|_| serde_json::json!(s))
}

/// 官方 update_fail_datas（模型 UpdateReadHistoryData.updateFailDatas）：
/// code==0 也可能带被拒明细，非空=进度上报被服务端拒绝，不当成功静默。
fn check_update_fails(v: &Value) -> AppResult<()> {
    match v
        .pointer("/data/update_fail_datas")
        .and_then(Value::as_array)
    {
        Some(fails) if !fails.is_empty() => Err(AppError::Media(format!(
            "历史上报被服务端拒绝 {} 条",
            fails.len()
        ))),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_report_uses_the_episode_index_only_in_history_update() {
        let series_id = "7697459724635098174";
        for (episode, vid) in [(1, 7697465535398677529_u64), (13, 7697465038512065561)] {
            let (update, upload) =
                watch_progress_bodies(series_id, &vid.to_string(), episode, 5000, 1);
            assert_eq!(update["update_datas"][0]["vid_index"], episode);
            assert_eq!(update["update_datas"][0]["vid"], vid);
            assert_eq!(upload["books"][0]["vid_index"], 0);
            assert_eq!(upload["books"][0]["item_id"], vid.to_string());
        }
    }

    #[test]
    fn guest_reports_keep_cumulative_playback_separate_from_episode_position() {
        let update = history_update_body("100", "113", 13, 0, 15_559, 1000, 0);
        assert_eq!(update["update_datas"][0]["current_play_position"], 0);
        assert_eq!(
            update["update_datas"][0]["player_accumulate_total_time"],
            15_559
        );
        assert_eq!(update["update_datas"][0]["update_timestamp_ms"], 0);

        let progress = guest_progress_item(&GuestProgressItem {
            series_id: "100".into(),
            vid: "113".into(),
            position_ms: 40_522,
            cumulative_ms: 72_097,
            observed_at_ms: 2000,
        });
        assert_eq!(progress["item_id"], "113");
        assert_eq!(progress["current_play_time"], 40_522);
        assert_eq!(progress["player_cumulative_total_duration"], 72_097);
        assert_eq!(progress["read_timestamp_ms"], 2000);
    }

    #[test]
    fn history_delete_converts_domain_to_zero_based() {
        assert_eq!(wire_vid_index(1), 0, "第 1 集 → 线上 0");
        assert_eq!(wire_vid_index(8), 7);
        assert_eq!(wire_vid_index(0), 0, "0/负值护栏");
        assert_eq!(wire_vid_index(-3), 0);
    }

    #[test]
    fn parse_history_maps_zero_based_wire() {
        let data = serde_json::json!({
            "data_list": [
                { "book_id_str": "111", "vid_index": 0, "current_play_position": 35000 },
                { "book_id_str": "222", "vid_index": 7 },
                { "book_id": 333 }
            ]
        });
        let page = parse_history(Some(&data)).unwrap();
        let vis: Vec<i64> = page.items.iter().map(|i| i.vid_index).collect();
        assert_eq!(vis, [1, 8, 1], "线上 0→第1集、7→第8集、缺席→第1集");
    }
}

#[cfg(test)]
pub(crate) mod probe {
    use super::*;

    #[test]
    fn update_fail_datas_non_empty_is_error() {
        let ok = serde_json::json!({ "code": 0, "data": { "update_fail_datas": [] } });
        assert!(check_update_fails(&ok).is_ok(), "空明细=成功");
        let absent = serde_json::json!({ "code": 0 });
        assert!(check_update_fails(&absent).is_ok(), "缺席字段=成功");
        let bad = serde_json::json!({ "code": 0, "data": { "update_fail_datas": [1, 2] } });
        assert!(check_update_fails(&bad).is_err(), "非空明细=被拒");
    }

    /// 云端历史真实返回验证（PROBE_DEVICE / PROBE_COOKIE 由真实库导出）。
    #[tokio::test]
    #[ignore = "直连真实接口的探测用例"]
    async fn probe_watch_history() {
        let device_json = std::env::var("PROBE_DEVICE").expect("PROBE_DEVICE");
        let account_cookie = std::env::var("PROBE_COOKIE").unwrap_or_default();
        let hg_cookie = std::env::var("PROBE_COOKIE_HG")
            .ok()
            .filter(|c| !c.is_empty());
        let hg_token = std::env::var("PROBE_TOKEN").ok().filter(|t| !t.is_empty());

        let device: hongguo_signer::device::DeviceProfile =
            serde_json::from_str(&device_json).expect("device json");
        let mut device = device;
        hongguo_signer::device::align_app_version(&mut device);
        let cookie_raw = hg_cookie.unwrap_or(account_cookie);
        let cookie = cookie_raw.replace(", ", "; ");
        let env = ApiEnv {
            proxy: hongguo_common::model::ProxyConfig::default(),
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

    fn chrono_like(ms: i64) -> String {
        let dt = ms / 1000 + 8 * 3600;
        let days = dt.div_euclid(86_400);
        let rem = dt.rem_euclid(86_400);
        format!("day+{days} {:02}:{:02}", rem / 3600, (rem % 3600) / 60)
    }
}

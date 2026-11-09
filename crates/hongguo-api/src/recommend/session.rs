//! 推荐会话态：运行时键、uuid、曝光环。

use serde_json::Value;

pub(super) fn session_runtime_pairs(uuid: &str) -> [(&'static str, String); 18] {
    [
        ("normal_session_cnt_in_day", "8".into()),
        ("cold_start_session_cnt_in_day", "4".into()),
        ("normal_session_id", format!("{uuid}#1")),
        ("cold_start_session_id", uuid.into()),
        ("cold_start_session_cnt_in_life", "32".into()),
        ("normal_session_cnt_in_life", "64".into()),
        ("charging", "0".into()),
        ("har_status", "0".into()),
        ("is_power_save_mode", "0".into()),
        ("app_dark_mode", "1".into()),
        ("screen_brightness", "21".into()),
        ("battery_pct", "71".into()),
        ("down_speed", "60000".into()),
        ("sys_dark_mode", "1".into()),
        ("app_mini_window", "0".into()),
        ("sys_mini_window", "1".into()),
        ("font_scale", "100".into()),
        ("network_type", "4".into()),
    ]
}

pub(super) const FEED_IMAGE_SHRINK: &str = "W3siaW1hZ2VfdHlwZSI6MiwiaW1hZ2Vfd2lkdGgiOjM2MCwic2hyaW5rX3R5cGUiOjJ9LHsiaW1hZ2VfdHlwZSI6MywiaW1hZ2Vfd2lkdGgiOjEwNzgsInNocmlua190eXBlIjozfSx7ImltYWdlX3R5cGUiOjQsImltYWdlX3dpZHRoIjo5OSwic2hyaW5rX3R5cGUiOjR9XQ==";

/// 官方 cell/change 的推荐补充上下文：base64 即
/// `{"recent_dislike_gid":[],"session_app_stay_time":0}`。
pub(super) const RECOMMEND_EXTRA: &str =
    "eyJyZWNlbnRfZGlzbGlrZV9naWQiOltdLCJzZXNzaW9uX2FwcF9zdGF5X3RpbWUiOjB9";

/// NPS 插入位配额（官方恒值）。
pub(super) const NPS_INSERT_RANK_JSON: &str = r#"{"1":3,"3":3,"2":3,"4":3}"#;

/// 进程生命周期的会话 UUID：官方一次启动发三枚，本仓用同一枚派生两形态
/// （裸 uuid 与 `uuid#1`）——服务端只要求同会话内一致。
pub(super) fn session_uuid() -> &'static str {
    static UUID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    UUID.get_or_init(uuid_v4)
}

/// 随机 UUID v4（不为此引入 uuid crate，rand 随机字节拼 8-4-4-4-12）。
fn uuid_v4() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = b[6] & 0x0f | 0x40; // version 4
    b[8] = b[8] & 0x3f | 0x80; // variant 10xx
    let hex = |range: std::ops::Range<usize>| {
        b[range]
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect::<String>()
    };
    format!(
        "{}-{}-{}-{}-{}",
        hex(0..4),
        hex(4..6),
        hex(6..8),
        hex(8..10),
        hex(10..16)
    )
}

/// 一条已下发条目的曝光记录：响应卡片 `cell_extra_data.rec_filter_info`
/// 的原样保留（content_type 字符串与 filter_id_map 不做映射猜测）。
#[derive(Clone)]
pub(super) struct RecImpression {
    pub(super) content_type: String,
    pub(super) rec_group_id: i64,
    pub(super) filter_id_map: serde_json::Value,
}

/// 最近曝光环容量：官方 latest_impression_gids 一次 2~6 个，取 8 留余量。
pub(super) const IMPRESSION_CAP: usize = 8;

type ImpressionRing = parking_lot::Mutex<Vec<RecImpression>>;
static RECENT_IMPRESSIONS: std::sync::OnceLock<ImpressionRing> = std::sync::OnceLock::new();

/// 记录一批曝光（翻页请求把它原样回传给服务端做个性化去重上下文）。
pub(super) fn push_impressions(items: impl IntoIterator<Item = RecImpression>) {
    let ring = RECENT_IMPRESSIONS.get_or_init(|| parking_lot::Mutex::new(Vec::new()));
    let mut guard = ring.lock();
    for item in items {
        if guard.iter().any(|i| i.rec_group_id == item.rec_group_id) {
            continue;
        }
        guard.push(item);
    }
    let excess = guard.len().saturating_sub(IMPRESSION_CAP);
    if excess > 0 {
        guard.drain(0..excess);
    }
}

pub(super) fn recent_impressions() -> Vec<RecImpression> {
    RECENT_IMPRESSIONS
        .get_or_init(|| parking_lot::Mutex::new(Vec::new()))
        .lock()
        .clone()
}

/// 曝光记录 → `rec_filter_info_list` 线格式（官方键序 content_type /
/// filter_id_map / rec_group_id，rec_group_id 是数字）。
pub(super) fn rec_filter_info_list(items: &[RecImpression]) -> serde_json::Value {
    serde_json::Value::Array(
        items
            .iter()
            .map(|i| {
                serde_json::json!({
                    "content_type": i.content_type,
                    "filter_id_map": i.filter_id_map,
                    "rec_group_id": i.rec_group_id,
                })
            })
            .collect(),
    )
}

/// 从响应卡片提取曝光记录；卡片不带 `cell_extra_data.rec_filter_info`
/// （部分运营卡）则返回 None，不猜测类型映射。
pub(super) fn rec_impression_of(raw: &Value) -> Option<RecImpression> {
    let info = raw.pointer("/cell_extra_data/rec_filter_info")?;
    let content_type = info
        .get("content_type")
        .and_then(Value::as_str)?
        .to_string();
    let rec_group_id = info.get("rec_group_id").and_then(Value::as_i64)?;
    Some(RecImpression {
        content_type,
        rec_group_id,
        filter_id_map: info.get("filter_id_map").cloned().unwrap_or(Value::Null),
    })
}

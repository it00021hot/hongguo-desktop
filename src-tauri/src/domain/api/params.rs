//! 官方 App 接口的请求体常量。
//!
//! ⚠️ 这些字段是**照抄现版**（`hongguo.js`）的实测值，改任何一个都会让服务端
//! 返回 `Code: 110001`（签名对但参数错）。验证时开 `RUST_LOG=debug`：
//! 响应字节数 > 0 只说明签名过了，还要看 body 里 `code` 是不是 0。

use serde_json::{Value, json};

/// 分集详情接口路径。
pub const DETAIL_PATH: &str = "/novel/player/multi_video_detail/preload/v1";

/// 清晰度列表（取流地址）接口路径。
pub const MODEL_PATH: &str = "/novel/player/multi_video_model/preload/v1";

/// 分集详情的 biz_param。
pub fn detail_biz_param() -> Value {
    json!({
        "detail_page_version": 0,
        "disable_digg_stat": false,
        "image_shrink_datas_str": "W3siaW1hZ2VfdHlwZSI6MywiaW1hZ2Vfd2lkdGgiOjkwMCwic2hyaW5rX3R5cGUiOjN9LHsiaW1hZ2VfdHlwZSI6NCwiaW1hZ2Vfd2lkdGgiOjcyLCJzaHJpbmtfdHlwZSI6NH1d\n",
        "need_all_video_definition": false,
        "need_mp4_align": false,
        "screen_width_px": "900",
        "source": 7,
        "use_os_player": false,
        "use_server_dns": false,
    })
}

/// 清晰度列表的 biz_param。
pub fn model_biz_param() -> Value {
    json!({
        "detail_page_version": 0,
        "device_level": 3,
        "disable_digg_stat": false,
        "need_all_video_definition": true,
        "need_mp4_align": false,
        "use_os_player": false,
        "use_server_dns": false,
        "video_platform": 1024,
    })
}

/// 构造分集详情请求体。
pub fn detail_payload(series_id: &str) -> Value {
    json!({
        "biz_param": detail_biz_param(),
        "dr_scene": "preload",
        "series_id": series_id,
    })
}

/// 构造取流地址请求体。
///
/// 平台用 `mixed_video_id_map` 传 vid，key 是「清晰度分组 id」，现版用 `"1004"`。
pub fn model_payload(vid: &str) -> Value {
    json!({
        "biz_param": model_biz_param(),
        "dr_scene": "preload",
        "mixed_video_id_map": { "1004": [vid] },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_have_no_trailing_slash() {
        // 尾部斜杠会被服务端当成不同接口
        assert!(!DETAIL_PATH.ends_with('/'));
        assert!(!MODEL_PATH.ends_with('/'));
        assert!(DETAIL_PATH.contains("/preload/v1"));
        assert!(MODEL_PATH.contains("/preload/v1"));
    }

    #[test]
    fn detail_payload_uses_preload_scene() {
        let p = detail_payload("123");
        assert_eq!(p["dr_scene"], "preload");
        assert_eq!(p["series_id"], "123");
        assert!(p["biz_param"]["image_shrink_datas_str"].is_string());
    }

    #[test]
    fn model_payload_wraps_vid_in_mixed_map() {
        let p = model_payload("v123");
        assert_eq!(p["mixed_video_id_map"]["1004"][0], "v123");
        assert_eq!(p["dr_scene"], "preload");
    }

    #[test]
    fn model_biz_param_requests_all_definitions() {
        assert_eq!(model_biz_param()["need_all_video_definition"], true);
    }
}

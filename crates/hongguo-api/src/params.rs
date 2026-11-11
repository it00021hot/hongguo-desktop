//! 官方 App 接口的请求体（静态类型承载）。
//! ⚠️ 字段值**照抄官方客户端**，改任何一个都会 110001 或 200+0；字段名一律
//! `#[serde(rename = "…")]` 显式声明服务端字面量（服务端命名风格不统一）。

use std::collections::BTreeMap;

use serde::Serialize;

/// commentapi 互动体系的枚举值（服务端契约：错一个数字即 103008「无社区功能」
/// 或 100103 PARAM_INVALID，三套体系互相只差一两个数字——集中声明，使用点只引用）。
pub mod interaction {
    /// `object_type`：互动对象类型。
    pub const OBJECT_TYPE_VIDEO: i64 = 6;
    pub const OBJECT_TYPE_REVIEW: i64 = 2;
    pub const OBJECT_TYPE_COMMENT: i64 = 8;
    /// 短剧（整部剧；追更的 do_action 对象）。
    pub const OBJECT_TYPE_SERIES: i64 = 10;

    /// `action_type`（articleapi do_action 动作码）：点赞 3/4 之外的动作。
    /// 追更 34 / 取消追更 35。
    pub const ACTION_TYPE_FOLLOW: i64 = 34;
    pub const ACTION_TYPE_UNFOLLOW: i64 = 35;

    /// 追更列表频道（get_user_action `action_channel`）。
    pub const ACTION_CHANNEL_FOLLOW: i64 = 9;

    /// `comment_type`：内容类型（剧评**发送**时是 0，列表 / 点赞是 2）。
    pub const COMMENT_TYPE_DANMAKU: i64 = 20;
    pub const COMMENT_TYPE_COMMENT: i64 = 4;
    pub const COMMENT_TYPE_REVIEW: i64 = 2;
    pub const COMMENT_TYPE_REVIEW_SEND: i64 = 0;

    /// `commit_source`：发送来源。
    pub const COMMIT_SOURCE_DANMAKU: i64 = 1500;
    pub const COMMIT_SOURCE_COMMENT: i64 = 3;
    pub const COMMIT_SOURCE_COMMENT_REPLY: i64 = 9;
    pub const COMMIT_SOURCE_REVIEW: i64 = 12;
    pub const COMMIT_SOURCE_REVIEW_REPLY: i64 = 13;

    /// `data_type`：负载类型（删除语义恒 9，与发送侧无关）。
    pub const DATA_TYPE_DANMAKU: i64 = 20;
    pub const DATA_TYPE_COMMENT: i64 = 4;
    pub const DATA_TYPE_REVIEW: i64 = 2;
    pub const DATA_TYPE_DELETE: i64 = 9;

    /// `group_type`：会话维度。
    pub const GROUP_TYPE_REVIEW: i64 = 1;
    pub const GROUP_TYPE_COMMENT: i64 = 30;

    /// `server_channel`：列表查询的服务端通道。
    pub const SERVER_CHANNEL_DANMAKU: i64 = 1000;
    pub const SERVER_CHANNEL_COMMENT: i64 = 18;
    pub const SERVER_CHANNEL_REVIEW: i64 = 34;

    /// `service_id`：删除接口的内容类型。
    pub const SERVICE_ID_REVIEW: i64 = 2;
    pub const SERVICE_ID_COMMENT: i64 = 4;

    /// 列表查询侧的 `comment_source`（与发送侧 `commit_source` 是两个枚举）。
    pub const LIST_SOURCE_DANMAKU: i64 = 601;
    pub const LIST_SOURCE_COMMENT: i64 = 4;
    pub const LIST_SOURCE_REVIEW: i64 = 1;
    pub const LIST_SOURCE_REVIEW_REPLIES: i64 = 501;
    pub const LIST_SOURCE_COMMENT_REPLIES: i64 = 504;
    /// 创作者主页的作品 tab（commentapi/user/profile：
    /// 详情页点创作者 → 主页，作品列表 tab_type=23 / sub_tab_type=22）
    pub const LIST_SOURCE_PROFILE_WORKS: i64 = 1506;
    pub const PROFILE_TAB_WORKS: i64 = 23;
    pub const PROFILE_SUB_TAB_WORKS: i64 = 22;
}

/// 表情域参数（列表端点用 `search_type` 分档，四档共用同一响应结构）。
pub mod emoticon {
    /// `search_type=1`：关键词搜索（`keyword` 带 `+` 前缀 = 前缀匹配）。
    pub const SEARCH_TYPE_SEARCH: i64 = 1;
    /// `search_type=2`：第三方表情库（GIF tab，分页 `offset`/`next_offset`）。
    pub const SEARCH_TYPE_THIRD_PARTY: i64 = 2;
    /// `search_type=3`：官方开面板并发拉的一档，收藏前后恒空、官方无入口——本仓不实现。
    pub const SEARCH_TYPE_COLLECTED: i64 = 3;
    /// `search_type=4`：我上传的自定义表情（固定 `keyword=profile`）。
    pub const SEARCH_TYPE_MINE: i64 = 4;

    /// 自定义表情的 `keyword` 取值（官方恒为 `profile`）。
    pub const OWN_EMOTICON_KEYWORD: &str = "profile";

    /// `collect_type`：1 = 添加（收藏为我的表情）。
    pub const COLLECT_TYPE_ADD: i64 = 1;
    /// `collect_type`：2 = 删除（从我的表情里移除）。
    pub const COLLECT_TYPE_DELETE: i64 = 2;

    /// `image_type`：1 = 静态图 / 2 = 动图。
    pub const IMAGE_TYPE_STATIC: i64 = 1;
    pub const IMAGE_TYPE_ANIMATED: i64 = 2;

    /// `business_param.emoji_scene_type`：2 = 第三方表情 / 4 = 自定义表情。
    /// 本地图片评论**不带**该字段。
    pub const SCENE_TYPE_THIRD_PARTY: i64 = 2;
    pub const SCENE_TYPE_OWN: i64 = 4;
}

/// 分集详情接口路径。
pub const DETAIL_PATH: &str = "/novel/player/multi_video_detail/preload/v1";

pub const PLAYER_MODEL_PATH: &str = "/novel/player/multi_video_model/player/v1";
const PRELOAD_MODEL_PATH: &str = "/novel/player/multi_video_model/preload/v1";

/// 单集取流路径。
pub const SINGLE_MODEL_PATH: &str = "/novel/player/video_model/v1/";

/// 详情页头部元信息端点。
pub const VIDEO_DETAIL_PATH: &str = "/novel/player/video_detail/v1/";

/// 分集详情 biz_param（字段名 = 服务端字面量）。
#[derive(Debug, Clone, Serialize)]
pub struct DetailBizParam {
    #[serde(rename = "detail_page_version")]
    pub detail_page_version: i64,
    #[serde(rename = "disable_digg_stat")]
    pub disable_digg_stat: bool,
    #[serde(rename = "image_shrink_datas_str")]
    pub image_shrink_datas_str: String,
    #[serde(rename = "need_all_video_definition")]
    pub need_all_video_definition: bool,
    #[serde(rename = "need_mp4_align")]
    pub need_mp4_align: bool,
    /// ⚠️ 官方形态是**字符串**（如 "1078"），不是数字。
    #[serde(rename = "screen_width_px")]
    pub screen_width_px: String,
    #[serde(rename = "source")]
    pub source: i64,
    #[serde(rename = "use_os_player")]
    pub use_os_player: bool,
    #[serde(rename = "use_server_dns")]
    pub use_server_dns: bool,
}

/// 分集详情请求体。
#[derive(Debug, Clone, Serialize)]
pub struct DetailPayload {
    #[serde(rename = "biz_param")]
    pub biz_param: DetailBizParam,
    #[serde(rename = "dr_scene")]
    pub dr_scene: &'static str,
    #[serde(rename = "series_id")]
    pub series_id: String,
}

/// 构造分集详情请求体。图片缩略规格与屏宽随设备档案
///（官方 1080 屏发 1081/99，宽度跟随设备屏宽，本仓档案 1078 与 tab/v 同源）。
pub fn detail_payload(series_id: &str) -> DetailPayload {
    DetailPayload {
        biz_param: DetailBizParam {
            detail_page_version: 0,
            disable_digg_stat: false,
            image_shrink_datas_str: "W3siaW1hZ2VfdHlwZSI6MywiaW1hZ2Vfd2lkdGgiOjEwNzgsInNocmlua190eXBlIjozfSx7ImltYWdlX3R5cGUiOjQsImltYWdlX3dpZHRoIjo5OSwic2hyaW5rX3R5cGUiOjR9XQ==".to_string(),
            need_all_video_definition: false,
            need_mp4_align: false,
            screen_width_px: "1078".to_string(),
            source: 7,
            use_os_player: false,
            use_server_dns: false,
        },
        dr_scene: "preload",
        series_id: series_id.to_string(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerModelBizParam {
    #[serde(rename = "detail_page_version")]
    pub detail_page_version: i64,
    #[serde(rename = "device_level")]
    pub device_level: i64,
    #[serde(rename = "disable_digg_stat")]
    pub disable_digg_stat: bool,
    #[serde(rename = "disable_video_relate_book")]
    pub disable_video_relate_book: bool,
    #[serde(rename = "need_all_video_definition")]
    pub need_all_video_definition: bool,
    #[serde(rename = "need_mp4_align")]
    pub need_mp4_align: bool,
    #[serde(rename = "use_os_player")]
    pub use_os_player: bool,
    #[serde(rename = "use_server_dns")]
    pub use_server_dns: bool,
    #[serde(rename = "video_platform")]
    pub video_platform: i64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerModelScene {
    Player,
    Preload,
}

impl PlayerModelScene {
    pub const fn path(self) -> &'static str {
        match self {
            Self::Player => PLAYER_MODEL_PATH,
            Self::Preload => PRELOAD_MODEL_PATH,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerModelPayload {
    #[serde(rename = "biz_param")]
    pub biz_param: PlayerModelBizParam,
    #[serde(rename = "dr_scene")]
    pub dr_scene: PlayerModelScene,
    #[serde(rename = "mixed_video_id_map")]
    pub mixed_video_id_map: BTreeMap<String, Vec<String>>,
}

pub fn player_model_payload(
    mixed_video_id_map: &BTreeMap<String, Vec<String>>,
    scene: PlayerModelScene,
) -> PlayerModelPayload {
    PlayerModelPayload {
        biz_param: player_model_biz_param(),
        dr_scene: scene,
        mixed_video_id_map: mixed_video_id_map.clone(),
    }
}

fn player_model_biz_param() -> PlayerModelBizParam {
    PlayerModelBizParam {
        detail_page_version: 0,
        device_level: 3,
        disable_digg_stat: false,
        disable_video_relate_book: false,
        need_all_video_definition: true,
        need_mp4_align: false,
        use_os_player: false,
        use_server_dns: false,
        video_platform: 1024,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SingleModelBizParam {
    #[serde(rename = "detail_page_version")]
    pub detail_page_version: i64,
    #[serde(rename = "device_level")]
    pub device_level: i64,
    #[serde(rename = "disable_digg_stat")]
    pub disable_digg_stat: bool,
    #[serde(rename = "disable_video_relate_book")]
    pub disable_video_relate_book: bool,
    #[serde(rename = "from_video_id")]
    pub from_video_id: String,
    #[serde(rename = "need_all_video_definition")]
    pub need_all_video_definition: bool,
    #[serde(rename = "need_mp4_align")]
    pub need_mp4_align: bool,
    #[serde(rename = "source")]
    pub source: i64,
    #[serde(rename = "use_os_player")]
    pub use_os_player: bool,
    #[serde(rename = "use_server_dns")]
    pub use_server_dns: bool,
    #[serde(rename = "video_platform")]
    pub video_platform: i64,
}

/// 官方单集取流请求体（端点 [`SINGLE_MODEL_PATH`]）。
#[derive(Debug, Clone, Serialize)]
pub struct SingleModelPayload {
    #[serde(rename = "biz_param")]
    pub biz_param: SingleModelBizParam,
    #[serde(rename = "content_type")]
    pub content_type: i64,
    #[serde(rename = "video_id")]
    pub video_id: String,
}

pub fn single_model_payload(video_id: &str, content_type: i64) -> SingleModelPayload {
    const PLAYER_SOURCE: i64 = 4;
    SingleModelPayload {
        biz_param: SingleModelBizParam {
            detail_page_version: 0,
            device_level: 3,
            disable_digg_stat: false,
            disable_video_relate_book: false,
            from_video_id: String::new(),
            need_all_video_definition: true,
            need_mp4_align: false,
            source: PLAYER_SOURCE,
            use_os_player: false,
            use_server_dns: false,
            video_platform: 3,
        },
        content_type,
        video_id: video_id.to_string(),
    }
}

/// 详情页头部元信息 biz_param。
#[derive(Debug, Clone, Serialize)]
pub struct VideoDetailBizParam {
    #[serde(rename = "caller_scene")]
    pub caller_scene: &'static str,
    #[serde(rename = "detail_page_version")]
    pub detail_page_version: i64,
    #[serde(rename = "disable_digg_stat")]
    pub disable_digg_stat: bool,
    #[serde(rename = "disable_video_relate_book")]
    pub disable_video_relate_book: bool,
    #[serde(rename = "from_video_id")]
    pub from_video_id: String,
    #[serde(rename = "need_all_video_definition")]
    pub need_all_video_definition: bool,
    #[serde(rename = "need_mp4_align")]
    pub need_mp4_align: bool,
    /// ⚠️ 字符串形态（同 [`DetailBizParam::screen_width_px`]）。
    #[serde(rename = "screen_width_px")]
    pub screen_width_px: String,
    #[serde(rename = "source")]
    pub source: i64,
    #[serde(rename = "use_os_player")]
    pub use_os_player: bool,
    #[serde(rename = "use_server_dns")]
    pub use_server_dns: bool,
    #[serde(rename = "video_id_type")]
    pub video_id_type: i64,
}

/// 详情页头部元信息请求体。
#[derive(Debug, Clone, Serialize)]
pub struct VideoDetailPayload {
    #[serde(rename = "biz_param")]
    pub biz_param: VideoDetailBizParam,
    #[serde(rename = "series_id")]
    pub series_id: String,
}

/// 构造详情页头部元信息请求体。
pub fn video_detail_payload(series_id: &str) -> VideoDetailPayload {
    VideoDetailPayload {
        biz_param: VideoDetailBizParam {
            caller_scene: "single_col",
            detail_page_version: 1,
            disable_digg_stat: false,
            disable_video_relate_book: false,
            from_video_id: String::new(),
            need_all_video_definition: false,
            need_mp4_align: false,
            screen_width_px: "1078".to_string(),
            source: 4,
            use_os_player: false,
            use_server_dns: false,
            video_id_type: 1,
        },
        series_id: series_id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_have_no_trailing_slash() {
        assert!(!DETAIL_PATH.ends_with('/'));
        assert!(DETAIL_PATH.contains("/preload/v1"));
        assert!(!PlayerModelScene::Preload.path().ends_with('/'));
        assert!(!PlayerModelScene::Player.path().ends_with('/'));
        assert!(SINGLE_MODEL_PATH.ends_with('/'));
    }

    #[test]
    fn player_model_payload_matches_captured_batch_body() {
        let group = BTreeMap::from([("1004".to_string(), vec!["123".to_string()])]);
        let body =
            serde_json::to_value(player_model_payload(&group, PlayerModelScene::Player)).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "biz_param": {
                    "detail_page_version": 0,
                    "device_level": 3,
                    "disable_digg_stat": false,
                    "disable_video_relate_book": false,
                    "need_all_video_definition": true,
                    "need_mp4_align": false,
                    "use_os_player": false,
                    "use_server_dns": false,
                    "video_platform": 1024
                },
                "dr_scene": "player",
                "mixed_video_id_map": {"1004": ["123"]}
            })
        );
        let preload =
            serde_json::to_value(player_model_payload(&group, PlayerModelScene::Preload)).unwrap();
        assert_eq!(preload["dr_scene"], "preload");
        assert_eq!(preload["biz_param"], body["biz_param"]);
        assert_eq!(preload["mixed_video_id_map"], body["mixed_video_id_map"]);
        assert_eq!(
            PlayerModelScene::Player.path(),
            "/novel/player/multi_video_model/player/v1"
        );
        assert_eq!(
            PlayerModelScene::Preload.path(),
            "/novel/player/multi_video_model/preload/v1"
        );
    }

    #[test]
    fn single_model_payload_matches_captured_body() {
        let body = serde_json::to_value(single_model_payload("vid-1", 1004)).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "biz_param": {
                    "detail_page_version": 0,
                    "device_level": 3,
                    "disable_digg_stat": false,
                    "disable_video_relate_book": false,
                    "from_video_id": "",
                    "need_all_video_definition": true,
                    "need_mp4_align": false,
                    "source": 4,
                    "use_os_player": false,
                    "use_server_dns": false,
                    "video_platform": 3
                },
                "content_type": 1004,
                "video_id": "vid-1"
            })
        );
    }

    #[test]
    fn detail_payload_uses_preload_scene() {
        let p = detail_payload("123");
        assert_eq!(p.dr_scene, "preload");
        assert_eq!(p.series_id, "123");
        assert!(p.biz_param.detail_page_version == 0);
    }

    #[test]
    fn payloads_serialize_with_server_field_names() {
        let d = serde_json::to_value(video_detail_payload("s1")).unwrap();
        assert_eq!(d["series_id"], "s1");
        assert_eq!(d["biz_param"]["video_id_type"], 1);
    }
}

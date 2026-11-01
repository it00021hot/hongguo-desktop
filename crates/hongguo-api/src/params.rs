//! 官方 App 接口的请求体（静态类型承载）。
//!
//! ⚠️ 这些字段是**照抄官方客户端**的值，改任何一个
//! 都会让服务端返回 `Code: 110001`（签名对但参数错）或 200+0。验证时开
//! `RUST_LOG=debug`：响应字节数 > 0 只说明签名过了，还要看 body 里 `code`
//! 是不是 0。
//!
//! 字段名一律 `#[serde(rename = "…")]` 显式声明服务端字面量
//! （不用 rename_all——服务端字段并非统一命名风格）。

use std::collections::BTreeMap;

use serde::Serialize;

/// commentapi 互动体系的枚举值。
///
/// ⚠️ 这些值是服务端契约：错一个数字，服务端就回
/// `103008「无社区功能」` 或 `100103 PARAM_INVALID`——而且**三套体系
/// （评论 / 剧评 / 弹幕）互相只差一两个数字**。历史上它们散落为字面量，
/// 被「顺手统一」的风险很高，所以集中声明，使用点只引用。
///
/// 字段速查：
/// - `object_type`（do_action 互动对象）：视频 6 / 剧评 2 / 评论 8
/// - `comment_type`（内容类型）：弹幕 20 / 评论 4 / 剧评 2；**发送剧评是 0**
/// - `commit_source`（发送来源）：弹幕 1500 / 评论 3 / 评论回复 9 /
///   剧评发送 12 / 剧评回复 13
/// - `data_type`（负载类型）：弹幕 20 / 评论 4 / 剧评 2；删除恒 9
/// - `group_type`（会话维度）：剧评 1 / 评论·弹幕 30
/// - `server_channel`（列表通道）：弹幕 1000 / 评论 18 / 剧评 34
/// - `service_id`（删除的内容类型）：剧评 2 / 评论·回复 4
/// - 列表侧 `comment_source`（与发送侧 `commit_source` 是**两个不同枚举**）：
///   弹幕 601 / 评论 4 / 剧评 1 / 剧评回复 501 / 评论回复 504
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

/// 表情域参数。
///
/// 表情列表端点 `/reading/ugc/comment/emoticon/v` 用 `search_type` 分档，
/// 四档共用同一响应结构。
pub mod emoticon {
    /// `search_type=1`：关键词搜索（`keyword` 带 `+` 前缀 = 前缀匹配）。
    pub const SEARCH_TYPE_SEARCH: i64 = 1;
    /// `search_type=2`：第三方表情库（GIF tab，分页 `offset`/`next_offset`）。
    pub const SEARCH_TYPE_THIRD_PARTY: i64 = 2;
    /// `search_type=3`：官方每次开表情面板都会并发拉的一档（`count=99`、
    /// 无 `keyword`）。收藏前后该档都返回空列表，官方
    /// 面板也没有对应入口——本仓不实现这一档。
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

// ---------------------------------------------------------------- 播放域路径

/// 分集详情接口路径。
pub const DETAIL_PATH: &str = "/novel/player/multi_video_detail/preload/v1";

/// 清晰度列表（取流地址）批量接口路径（2026-11-01 竞品实流抓包）。
///
/// ⚠️ 这是当前**唯一活着的取流端点**：reading 系**无签名**轻请求
/// （见 [`crate::client::api_call_model_light`]），body 见
/// [`model_light_payload`]。旧路径 `multi_video_model/player/v1` 与
/// 单集 `video_model/v1/` 已整体回 200+0（2026-11-01 实测，两种形态
/// 无论签名/身份/版本代际如何均不发货）。
pub const MODEL_LIGHT_PATH: &str = "/novel/player/multi_video_model/v1/";

/// 详情页头部元信息端点。
pub const VIDEO_DETAIL_PATH: &str = "/novel/player/video_detail/v1/";

// ---------------------------------------------------------------- 详情 biz_param

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

/// 构造分集详情请求体。
///
/// 图片缩略规格与屏宽随设备档案（
/// 结构 [type3=大图/type4=小图]，官方 1080 屏发 1081/99——宽度跟随设备
/// 屏宽，本仓档案 1078 与 tab/v 同源）。
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

// ---------------------------------------------------------------- 取流 biz_param

/// 批量取流请求体（2026-11-01 竞品实流抓包 1:1）：
/// `biz_param` 固定八字段（**无 `dr_scene`**）+ `mixed_video_id_map`。
#[derive(Debug, Clone, Serialize)]
pub struct ModelLightPayload {
    #[serde(rename = "biz_param")]
    pub biz_param: ModelLightBizParam,
    #[serde(rename = "mixed_video_id_map")]
    pub mixed_video_id_map: BTreeMap<String, Vec<String>>,
}

/// 轻请求体 biz_param——键集与取值与抓包体逐键一致
/// （`device_level:4`、`use_server_dns:false`；老形态的
/// `dr_scene/disable_video_relate_book/use_server_dns_scene` 键不存在）。
#[derive(Debug, Clone, Serialize)]
pub struct ModelLightBizParam {
    #[serde(rename = "detail_page_version")]
    pub detail_page_version: i64,
    #[serde(rename = "device_level")]
    pub device_level: i64,
    #[serde(rename = "disable_digg_stat")]
    pub disable_digg_stat: bool,
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

/// 构造批量取流请求体（端点 [`MODEL_LIGHT_PATH`]）。
///
/// `mixed_video_id_map` 的 key 是**条目 content_type 的原样字符串**
/// （key 错配 → 不发货；漫剧 feed ct=1004→"1004"、真人剧 ct=1→"1"、
/// 书城 cell 条目 1001→"1001"）。
pub fn model_light_payload(
    mixed_video_id_map: &BTreeMap<String, Vec<String>>,
) -> ModelLightPayload {
    ModelLightPayload {
        biz_param: ModelLightBizParam {
            detail_page_version: 0,
            device_level: 4,
            disable_digg_stat: false,
            need_all_video_definition: true,
            need_mp4_align: false,
            use_os_player: false,
            use_server_dns: false,
            video_platform: 1024,
        },
        mixed_video_id_map: mixed_video_id_map.clone(),
    }
}

// ---------------------------------------------------------------- 详情页头部 meta

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
        // 尾部斜杠会被服务端当成不同接口（MODEL_LIGHT_PATH 的尾斜杠
        // 是抓包体自带，属于端点身份的一部分，不在此列）
        assert!(!DETAIL_PATH.ends_with('/'));
        assert!(DETAIL_PATH.contains("/preload/v1"));
        assert!(MODEL_LIGHT_PATH.ends_with('/'));
        assert!(MODEL_LIGHT_PATH.contains("/multi_video_model/v1/"));
        // 旧的 dr_scene 段（multi_video_model/player/v1）已死，不得回潮
        assert!(!MODEL_LIGHT_PATH.contains("player/v1"));
    }

    #[test]
    fn model_light_payload_matches_capture() {
        // 轻请求批量体（2026-11-01 竞品实流抓包）：biz_param 八键、
        // 无 dr_scene、分组原样回填
        let map = BTreeMap::from([
            ("1004".to_string(), vec!["v1".to_string()]),
            ("1".to_string(), vec!["v2".to_string()]),
        ]);
        let p = model_light_payload(&map);
        assert_eq!(p.mixed_video_id_map.len(), 2);
        let body = serde_json::to_value(&p).unwrap();
        assert!(body.get("dr_scene").is_none(), "轻请求体无 dr_scene 键");
        assert!(
            body.pointer("/biz_param/from_video_id").is_none()
                && body.pointer("/biz_param/source").is_none()
                && body
                    .pointer("/biz_param/disable_video_relate_book")
                    .is_none()
                && body.pointer("/biz_param/use_server_dns_scene").is_none(),
            "biz_param 键集与抓包体逐键一致"
        );
        assert_eq!(body["biz_param"]["device_level"], 4);
        assert_eq!(body["biz_param"]["video_platform"], 1024);
        assert!(!body["biz_param"]["use_server_dns"].as_bool().unwrap());
    }

    #[test]
    fn detail_payload_uses_preload_scene() {
        let p = detail_payload("123");
        assert_eq!(p.dr_scene, "preload");
        assert_eq!(p.series_id, "123");
        assert!(p.biz_param.detail_page_version == 0);
    }

    #[test]
    fn model_light_payload_keeps_groups_verbatim() {
        // 分组原样回填（key = 条目 content_type 原样串，不重排不映射）
        let p = model_light_payload(&BTreeMap::from([
            ("1".to_string(), vec!["v1".to_string()]),
            (
                "1004".to_string(),
                vec!["v123".to_string(), "v456".to_string()],
            ),
        ]));
        let vids = p.mixed_video_id_map.get("1004").expect("分组 1004 在场");
        assert_eq!(vids, &["v123".to_string(), "v456".to_string()]);
        assert!(p.mixed_video_id_map.contains_key("1"));
        assert_eq!(p.biz_param.video_platform, 1024);
    }

    #[test]
    fn payloads_serialize_with_server_field_names() {
        let p = serde_json::to_value(model_light_payload(&BTreeMap::from([(
            "1".to_string(),
            vec!["v1".to_string()],
        )])))
        .unwrap();
        assert!(p.get("biz_param").is_some());
        assert!(p.get("mixed_video_id_map").is_some());
        assert!(p["biz_param"].get("need_all_video_definition").is_some());
        let d = serde_json::to_value(video_detail_payload("s1")).unwrap();
        assert_eq!(d["series_id"], "s1");
        assert_eq!(d["biz_param"]["video_id_type"], 1);
    }
}

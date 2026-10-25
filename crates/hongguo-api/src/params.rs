//! 官方 App 接口的请求体（静态类型承载）。
//!
//! ⚠️ 这些字段是**照抄官方客户端**的实测值，改任何一个
//! 都会让服务端返回 `Code: 110001`（签名对但参数错）或 200+0。验证时开
//! `RUST_LOG=debug`：响应字节数 > 0 只说明签名过了，还要看 body 里 `code`
//! 是不是 0。
//!
//! 字段名一律 `#[serde(rename = "…")]` 显式声明服务端字面量（逐值对齐
//! 抓包，不用 rename_all——服务端字段并非统一命名风格）。

use std::collections::BTreeMap;

use serde::Serialize;

/// commentapi 互动体系的枚举值。
///
/// ⚠️ 这些值是抓包锁定的服务端契约：错一个数字，服务端就回
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
    /// 短剧（整部剧；追更的 do_action 对象，2026-10-15 抓 player v1.2.1 实包）。
    pub const OBJECT_TYPE_SERIES: i64 = 10;

    /// `action_type`（articleapi do_action 动作码）：点赞 3/4 之外的动作。
    /// 追更 34 / 取消追更 35（2026-10-15 抓 player 实包往返三次锁定）。
    pub const ACTION_TYPE_FOLLOW: i64 = 34;
    pub const ACTION_TYPE_UNFOLLOW: i64 = 35;

    /// 追更列表频道（get_user_action `action_channel`，抓包锁定）。
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
    /// 创作者主页的作品 tab（commentapi/user/profile，2026-10-15 官方实包：
    /// 详情页点创作者 → 主页，作品列表 tab_type=23 / sub_tab_type=22）
    pub const LIST_SOURCE_PROFILE_WORKS: i64 = 1506;
    pub const PROFILE_TAB_WORKS: i64 = 23;
    pub const PROFILE_SUB_TAB_WORKS: i64 = 22;
}

// ---------------------------------------------------------------- 播放域路径

/// 分集详情接口路径。
pub const DETAIL_PATH: &str = "/novel/player/multi_video_detail/preload/v1";

/// 清晰度列表（取流地址）批量接口路径（dr_scene=player，起播/切剧形态）。
///
/// 2026-10-25 抓官方 App 7.3.9.32 实流：起播时刻打本端点（6 vid：当前 +
/// 信息流后续，dr_scene=player）；纯滑动预取另有 dr_scene=preload 形态
/// （端点 `/novel/player/multi_video_model/preload/v1`，biz_param 全同，
/// 本仓无该场景不打）。剧内切集/单集取流走 [`SINGLE_MODEL_PATH`]。
///
/// ⚠️ 必须打 **lq 域**（`crate::client::LQ_API_ORIGIN`）。批量形态
/// 2026-10-14 曾对全新注册设备回 200+0，2026-10-25 复测恢复发货（直连
/// probe 6 vid 全命中）。
pub const PLAYER_MODEL_PATH: &str = "/novel/player/multi_video_model/player/v1";

/// 单集取流端点，2026-10-14 实测现有设备直接可用
/// （code=0）。
pub const SINGLE_MODEL_PATH: &str = "/novel/player/video_model/v1/";

/// 详情页头部元信息端点（2026-10-07 抓包详情页锁定）。
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
    /// ⚠️ 抓包形态是**字符串**（如 "1078"），不是数字。
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
/// 图片缩略规格与屏宽随设备档案（2026-10-25 抓官方 App 7.3.9.32 实流：
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

/// 取流 biz_param 的公共字段（批量/单集只差 `video_platform`、`source`、
/// `from_video_id` 是否在场，见各自 payload）。
#[derive(Debug, Clone, Serialize)]
pub struct ModelBizParam {
    #[serde(rename = "detail_page_version")]
    pub detail_page_version: i64,
    #[serde(rename = "device_level")]
    pub device_level: i64,
    #[serde(rename = "disable_digg_stat")]
    pub disable_digg_stat: bool,
    #[serde(rename = "disable_video_relate_book")]
    pub disable_video_relate_book: bool,
    /// 仅单集形态在场（官方切集实流 `""`）；批量体无此键。
    #[serde(rename = "from_video_id", skip_serializing_if = "Option::is_none")]
    pub from_video_id: Option<String>,
    #[serde(rename = "need_all_video_definition")]
    pub need_all_video_definition: bool,
    #[serde(rename = "need_mp4_align")]
    pub need_mp4_align: bool,
    #[serde(rename = "use_os_player")]
    pub use_os_player: bool,
    #[serde(rename = "use_server_dns")]
    pub use_server_dns: bool,
    /// 仅单集形态在场（官方切集实流 `4`）；批量体无此键。
    #[serde(rename = "source", skip_serializing_if = "Option::is_none")]
    pub source: Option<i64>,
    /// 批量 1024 / 单集 3（官方两形态实流一致）。
    #[serde(rename = "video_platform")]
    pub video_platform: i64,
}

/// 批量取流请求体（`dr_scene` 与端点同名 "player"）。
///
/// `mixed_video_id_map` 的 key 是「清晰度分组 id」，现版用 `"1004"`；
/// 值是「当前 + 信息流后续几部」的 vid 列表（一次 ~5 个）。
#[derive(Debug, Clone, Serialize)]
pub struct ModelPayload {
    #[serde(rename = "biz_param")]
    pub biz_param: ModelBizParam,
    #[serde(rename = "dr_scene")]
    pub dr_scene: &'static str,
    #[serde(rename = "mixed_video_id_map")]
    pub mixed_video_id_map: BTreeMap<String, Vec<String>>,
}

/// 构造批量请求体（预取/真播共用，仅 dr_scene 不同——两个场景的
/// biz_param 逐字段一致，2026-10-25 官方体对照）。
fn batch_model_payload(
    mixed_video_id_map: &BTreeMap<String, Vec<String>>,
    dr_scene: &'static str,
) -> ModelPayload {
    ModelPayload {
        biz_param: batch_model_biz_param(),
        dr_scene,
        mixed_video_id_map: mixed_video_id_map.clone(),
    }
}

/// 构造批量**真播/起播**请求体（端点 [`PLAYER_MODEL_PATH`]，dr_scene=player）。
///
/// 官方两处批量（2026-10-25 抓 7.3.9.32 实流）：**起播/切剧时刻**用本形态
/// （6 vid：当前 + 信息流后续）；纯滑动预取另有 dr_scene=preload 形态
/// （端点 `/novel/player/multi_video_model/preload/v1`，biz_param 全同），
/// 本仓预取挂在起播时刻，统一走本形态。
///
/// `mixed_video_id_map` 的 key 是**条目 content_type 的原样字符串**
/// （直连 probe 实证：key 错配 → 空响应；漫剧 feed ct=1004→"1004"、
/// 真人剧 ct=1→"1"、书城 cell 条目 1001→"1001"）。
pub fn player_model_payload(mixed_video_id_map: &BTreeMap<String, Vec<String>>) -> ModelPayload {
    batch_model_payload(mixed_video_id_map, "player")
}

/// 批量形态（player/preload 共用）的 biz_param——官方体无 `from_video_id`
/// 与 `source` 键。
fn batch_model_biz_param() -> ModelBizParam {
    ModelBizParam {
        detail_page_version: 0,
        device_level: 3,
        disable_digg_stat: false,
        disable_video_relate_book: false,
        from_video_id: None,
        need_all_video_definition: true,
        need_mp4_align: false,
        source: None,
        use_os_player: false,
        use_server_dns: false,
        video_platform: 1024,
    }
}

/// 单集取流请求体（2026-10-25 抓官方 App 切集实流逐值对齐）：
/// `video_platform:3`（批量是 1024）、`from_video_id:""` 与 `source:4`
/// 在场（批量体两键均无）。
///
/// ⚠️ `content_type` 是**这一集所属剧的真实类型**（官方切集实流：漫剧
/// 1004、真人剧 1、书城条目 1001，随剧变，不是协议常量）——调用方从
/// 注册表取，见 `crate::play_url::content_type_of`。
#[derive(Debug, Clone, Serialize)]
pub struct SingleModelPayload {
    #[serde(rename = "biz_param")]
    pub biz_param: ModelBizParam,
    #[serde(rename = "content_type")]
    pub content_type: i64,
    #[serde(rename = "video_id")]
    pub video_id: String,
}

/// 单集请求 `content_type` 的兜底值：注册表查不到类型时只能填一个，
/// 服务端对该字段容错（错值实测照常返回流表）；1004 是早期探索时代的
/// 历史值，仅作未知类型的占位，已知类型一律走注册表真值。
pub const SINGLE_CONTENT_TYPE_FALLBACK: i64 = 1004;

/// 构造单集取流请求体。
pub fn single_model_payload(vid: &str, content_type: i64) -> SingleModelPayload {
    SingleModelPayload {
        biz_param: ModelBizParam {
            detail_page_version: 0,
            device_level: 3,
            disable_digg_stat: false,
            disable_video_relate_book: false,
            from_video_id: Some(String::new()),
            need_all_video_definition: true,
            need_mp4_align: false,
            source: Some(4),
            use_os_player: false,
            use_server_dns: false,
            video_platform: 3,
        },
        content_type,
        video_id: vid.to_string(),
    }
}

// ---------------------------------------------------------------- 详情页头部 meta

/// 详情页头部元信息 biz_param（2026-10-07 抓包逐值对齐）。
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
        // 尾部斜杠会被服务端当成不同接口
        assert!(!DETAIL_PATH.ends_with('/'));
        assert!(!PLAYER_MODEL_PATH.ends_with('/'));
        assert!(DETAIL_PATH.contains("/preload/v1"));
        assert!(PLAYER_MODEL_PATH.contains("/player/v1"));
    }

    #[test]
    fn player_model_payload_scene_and_grouping() {
        // 官方起播批量体（2026-10-25 实流）：dr_scene=player + 分组原样
        // 回填 + 无 from_video_id / source 键
        let map = BTreeMap::from([
            ("1004".to_string(), vec!["v1".to_string()]),
            ("1".to_string(), vec!["v2".to_string()]),
        ]);
        let p = player_model_payload(&map);
        assert_eq!(p.dr_scene, "player");
        assert_eq!(p.mixed_video_id_map.len(), 2);
        let body = serde_json::to_value(&p).unwrap();
        assert!(
            body.pointer("/biz_param/from_video_id").is_none(),
            "官方批量体无 from_video_id 键"
        );
        assert!(
            body.pointer("/biz_param/source").is_none(),
            "官方批量体无 source 键"
        );
        assert_eq!(body["biz_param"]["video_platform"], 1024);
    }

    #[test]
    fn detail_payload_uses_preload_scene() {
        let p = detail_payload("123");
        assert_eq!(p.dr_scene, "preload");
        assert_eq!(p.series_id, "123");
        assert!(p.biz_param.detail_page_version == 0);
    }

    #[test]
    fn player_model_payload_keeps_groups_verbatim() {
        // 分组原样回填（key = 条目 content_type 原样串，不重排不映射）
        let p = player_model_payload(&BTreeMap::from([
            ("1".to_string(), vec!["v1".to_string()]),
            (
                "1004".to_string(),
                vec!["v123".to_string(), "v456".to_string()],
            ),
        ]));
        let vids = p.mixed_video_id_map.get("1004").expect("分组 1004 在场");
        assert_eq!(vids, &["v123".to_string(), "v456".to_string()]);
        assert!(p.mixed_video_id_map.contains_key("1"));
        assert!(!p.biz_param.disable_video_relate_book);
        assert_eq!(p.biz_param.video_platform, 1024);
    }

    #[test]
    fn single_model_payload_matches_capture() {
        // 2026-10-25 官方切集实流：content_type=剧类型（动态）、source:4 在场
        let p = single_model_payload("v1", 1001);
        assert_eq!(p.content_type, 1001);
        assert_eq!(p.video_id, "v1");
        assert_eq!(p.biz_param.video_platform, 3);
        assert_eq!(p.biz_param.from_video_id, Some(String::new()));
        assert_eq!(p.biz_param.source, Some(4));
        // 序列化形态：批量体缺席的两键在单集体在场（官方切集体逐字段）
        let body = serde_json::to_value(&p).unwrap();
        assert_eq!(body["biz_param"]["source"], 4);
        assert_eq!(body["biz_param"]["from_video_id"], "");
    }

    #[test]
    fn payloads_serialize_with_server_field_names() {
        let p = serde_json::to_value(single_model_payload("v1", 1004)).unwrap();
        assert!(p.get("biz_param").is_some());
        assert!(p.get("content_type").is_some());
        assert!(p["biz_param"].get("need_all_video_definition").is_some());
        assert!(p["biz_param"].get("disable_video_relate_book").is_some());
        let d = serde_json::to_value(video_detail_payload("s1")).unwrap();
        assert_eq!(d["series_id"], "s1");
        assert_eq!(d["biz_param"]["video_id_type"], 1);
    }
}

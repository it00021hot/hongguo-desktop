//! 清晰度择优。
//!
//! 从 `video_list` 里挑出最该下的那一条。
//!
//! ⚠️ 流的元数据全在 `video_meta` 子对象里，且 `definition` 带 `p` 后缀
//!    （如 `"1080p"`）。顶层没有 `definition` / `bitrate` / 宽高——
//!    照着旧 schema 读会全部取到 0，择优退化成「永远选第一条」（360p）。

use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde_json::Value;

/// 流的可比较指标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamScore {
    /// 清晰度等级
    pub def_num: u32,
    /// 像素数
    pub pixels: u32,
    /// 编码器偏好（**越小越优先**：h265/h264 系 > bytevc1 > 其它）
    pub codec_rank: u8,
    /// 码率
    pub bitrate: u32,
}

/// 择优顺序：清晰度 > 像素 > 编码器偏好 > 码率。
///
/// 编码器一项要**反向**比较——rank 越小越好，而 `Ord` 的语义是「大者胜」，
/// 直接用 derive 会把 bytevc1 排在 h265 前面。
impl Ord for StreamScore {
    fn cmp(&self, other: &Self) -> Ordering {
        self.def_num
            .cmp(&other.def_num)
            .then_with(|| self.pixels.cmp(&other.pixels))
            .then_with(|| other.codec_rank.cmp(&self.codec_rank))
            .then_with(|| self.bitrate.cmp(&other.bitrate))
    }
}

impl PartialOrd for StreamScore {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// 编码器偏好排序。
///
/// 平台优先给 `h265` / `h264`，其次 `bytevc1`，其余（含 `bytevc2`）最后。
/// 同一清晰度下优先选标准编码器——ffmpeg 与软解链路对它们的兼容性差异很大。
fn codec_rank(codec: &str) -> u8 {
    match codec {
        "h265" | "h264" | "h265_hvc1" | "hevc" | "hvc1" | "avc1" => 0,
        "bytevc1" => 1,
        _ => 2,
    }
}

/// 计算一条流的分数。
pub fn stream_score(v: &Value) -> StreamScore {
    let meta = v.get("video_meta");

    let definition = meta
        .and_then(|m| m.get("definition"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    let dim = |key: &str| -> u32 {
        meta.and_then(|m| m.get(key))
            .and_then(Value::as_i64)
            .unwrap_or(0) as u32
    };

    let codec = meta
        .and_then(|m| m.get("codec_type"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    let bitrate = meta
        .and_then(|m| m.get("bitrate").or_else(|| m.get("real_bitrate")))
        .and_then(Value::as_i64)
        .unwrap_or(0) as u32;

    StreamScore {
        def_num: parse_definition(definition),
        pixels: dim("vwidth").saturating_mul(dim("vheight")),
        codec_rank: codec_rank(codec),
        bitrate,
    }
}

/// 解析 `definition` 为数值。
///
/// 优先取 `"1080p"` 里的数字；取不到时按常见档位名映射。
fn parse_definition(s: &str) -> u32 {
    if let Some(v) = definition_digits(s) {
        return v;
    }
    match s.to_ascii_lowercase().as_str() {
        "4k" => 2160,
        "2k" => 1440,
        "fhd" | "fullhd" => 1080,
        "hd" => 720,
        "sd" => 480,
        "ld" => 360,
        _ => 0,
    }
}

/// 从 `"1080p"` 这类字符串里取数字部分。
///
/// 对应 JS 的 `/(\d+)\s*p/i`：数字后面**必须**跟可选空白再跟一个 `p`。
/// 不带这个约束的话 `"4K"` 会被截成 `4`，落到 2160 档的映射就永远走不到。
fn definition_digits(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    let start = bytes.iter().position(u8::is_ascii_digit)?;
    let digits_end = bytes[start..]
        .iter()
        .position(|b| !b.is_ascii_digit())
        .map_or(bytes.len(), |i| start + i);

    let mut tail = digits_end;
    while tail < bytes.len() && bytes[tail].is_ascii_whitespace() {
        tail += 1;
    }
    if bytes.get(tail)?.eq_ignore_ascii_case(&b'p') {
        s[start..digits_end].parse().ok()
    } else {
        None
    }
}

/// 一集实际提供的清晰度档位，供前端渲染切换菜单。
///
/// 就是 [`crate::domain::model::VideoDefinition`]：取流与 IPC 两头都要用，
/// 另立一个类型只会在中间多一层转换。
pub use crate::domain::model::VideoDefinition as Definition;

/// 列出一集提供的全部清晰度，按高到低。
///
/// 同一档位通常有多条流（h265 / bytevc1 / bytevc2 各一条），菜单按档位
/// 给一项即可，具体选哪条编码仍由 [`pick_stream_at`] 的择优规则决定。
pub fn list_definitions(model: &Value) -> Vec<Definition> {
    let Some(list) = model.get("video_list").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut seen: BTreeSet<Definition> = BTreeSet::new();
    for v in list {
        if v.get("main_url").and_then(Value::as_str).is_none() {
            continue;
        }
        let score = stream_score(v);
        // def_num 为 0 说明元数据缺失，列出来只会让菜单多一个看不懂的项
        if score.def_num == 0 {
            continue;
        }
        let meta = v.get("video_meta");
        let dim = |key: &str| -> u32 {
            meta.and_then(|m| m.get(key))
                .and_then(Value::as_i64)
                .unwrap_or(0) as u32
        };
        seen.insert(Definition {
            value: score.def_num,
            width: dim("vwidth"),
            height: dim("vheight"),
        });
    }

    // BTreeSet 已按 (档位, 宽, 高) 升序，反转即高到低
    seen.into_iter().rev().collect()
}

/// 挑到的一条流。
pub struct PlayStream {
    pub url: String,
    pub spade: String,
    pub codec: Option<String>,
    /// 实际选中的档位。发生回退时它是自动挑的那档，不是请求的那档。
    pub definition: u32,
}

/// 从 `video_model` 里挑指定清晰度的那条流。
///
/// `target` 为 `None` 时挑最高档（沿用 [`StreamScore`] 的完整择优规则）；
/// 指定档位时在该档位内择优——同一档位的多条流编码器不同，仍要按
/// 编码器偏好 > 码率 挑出最好的那条。
pub fn pick_stream_at(model: &Value, target: Option<u32>) -> Option<PlayStream> {
    let list = model.get("video_list")?.as_array()?;
    let mut best: Option<(StreamScore, &Value)> = None;
    for v in list {
        if v.get("main_url").and_then(Value::as_str).is_none() {
            continue;
        }
        let score = stream_score(v);
        if target.is_some_and(|t| score.def_num != t) {
            continue;
        }
        if best.as_ref().is_none_or(|(b, _)| score > *b) {
            best = Some((score, v));
        }
    }

    // 指定了档位却一条都没匹配上：退回最高档。
    // 否则「切到平台没给的那档」会直接变成播放失败，而自动回退对用户是无感的。
    if best.is_none() && target.is_some() {
        return pick_stream_at(model, None);
    }

    let (score, v) = best?;
    Some(PlayStream {
        url: v.get("main_url")?.as_str()?.to_string(),
        spade: v
            .get("encrypt_info")
            .and_then(|e| e.get("spade_a"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        codec: v
            .get("video_meta")
            .and_then(|m| m.get("codec_type"))
            .and_then(Value::as_str)
            .map(str::to_string),
        definition: score.def_num,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 实网样本：bytevc2 各档位 + bytevc1 1080p。
    /// 元数据全在 `video_meta` 里，顶层没有 definition / bitrate。
    fn real_model() -> Value {
        json!({
            "video_list": [
                { "main_url": "https://cdn/360.mp4",
                  "video_meta": { "definition": "360p", "vwidth": 360, "vheight": 640,
                                  "bitrate": 156621, "codec_type": "bytevc2" } },
                { "main_url": "https://cdn/720.mp4",
                  "video_meta": { "definition": "720p", "vwidth": 720, "vheight": 1280,
                                  "bitrate": 304444, "codec_type": "bytevc2" } },
                { "main_url": "https://cdn/1080.mp4",
                  "encrypt_info": { "spade_a": "KEY1080" },
                  "video_meta": { "definition": "1080p", "vwidth": 1080, "vheight": 1920,
                                  "bitrate": 503006, "codec_type": "bytevc1" } }
            ]
        })
    }

    #[test]
    fn picks_highest_definition_from_real_schema() {
        let s = pick_stream_at(&real_model(), None).unwrap();
        assert_eq!(s.url, "https://cdn/1080.mp4");
        assert_eq!(s.spade, "KEY1080");
        assert_eq!(s.codec.as_deref(), Some("bytevc1"));
        assert_eq!(s.definition, 1080);
    }

    #[test]
    fn lists_every_definition_high_to_low() {
        let defs = list_definitions(&real_model());
        assert_eq!(
            defs.iter().map(|d| d.value).collect::<Vec<_>>(),
            vec![1080, 720, 360],
            "菜单应按高到低列出全部档位"
        );
        assert_eq!((defs[0].width, defs[0].height), (1080, 1920));
    }

    #[test]
    fn picks_the_requested_definition() {
        let model = real_model();
        let s = pick_stream_at(&model, Some(720)).expect("720p 应可选");
        assert_eq!(s.url, "https://cdn/720.mp4");
        assert_eq!(s.definition, 720);
    }

    #[test]
    fn unavailable_definition_falls_back_to_the_best() {
        // 平台没给 4K 时静默回退到最高档，而不是让播放直接失败
        let s = pick_stream_at(&real_model(), Some(2160)).expect("应回退而不是报错");
        assert_eq!(s.definition, 1080);
    }

    #[test]
    fn definitions_without_metadata_are_skipped() {
        // def_num 解析不出来的流不该出现在菜单里，否则会多出一个 0p 的怪项
        let model = serde_json::json!({
            "video_list": [
                { "main_url": "a", "video_meta": { "definition": "1080p", "vwidth": 1920, "vheight": 1080 } },
                { "main_url": "b", "video_meta": {} }
            ]
        });
        let defs = list_definitions(&model);
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].value, 1080);
    }

    #[test]
    fn score_reads_definition_with_p_suffix() {
        let model = real_model();
        let scores: Vec<StreamScore> = model["video_list"]
            .as_array()
            .unwrap()
            .iter()
            .map(stream_score)
            .collect();
        assert_eq!(
            scores.iter().map(|s| s.def_num).collect::<Vec<_>>(),
            vec![360, 720, 1080],
            "definition 带 p 后缀也要能解析出数字"
        );
        assert_eq!(scores[0].pixels, 360 * 640);
        assert_eq!(scores[2].pixels, 1080 * 1920);
        assert_eq!(scores[2].bitrate, 503006);
    }

    #[test]
    fn named_definitions_map_to_numbers() {
        assert_eq!(parse_definition("4K"), 2160);
        assert_eq!(parse_definition("FHD"), 1080);
        assert_eq!(parse_definition("hd"), 720);
        assert_eq!(parse_definition("sd"), 480);
        assert_eq!(parse_definition(""), 0);
        assert_eq!(parse_definition("未知"), 0);
    }

    #[test]
    fn bitrate_falls_back_to_real_bitrate() {
        let v = json!({ "video_meta": { "real_bitrate": 88888, "codec_type": "h265" } });
        assert_eq!(stream_score(&v).bitrate, 88888);
    }

    #[test]
    fn prefers_standard_codec_at_same_definition() {
        let model = json!({
            "video_list": [
                { "main_url": "bytevc", "video_meta": { "definition": "1080p", "vwidth": 1080,
                    "vheight": 1920, "bitrate": 500000, "codec_type": "bytevc1" } },
                { "main_url": "h265", "video_meta": { "definition": "1080p", "vwidth": 1080,
                    "vheight": 1920, "bitrate": 500000, "codec_type": "h265" } }
            ]
        });
        let s = pick_stream_at(&model, None).unwrap();
        assert_eq!(s.url, "h265", "同清晰度同码率应优先标准编码器");
    }

    #[test]
    fn skips_entries_without_url() {
        let model = json!({
            "video_list": [
                { "video_meta": { "definition": "1080p" } },
                { "main_url": "only-one", "video_meta": { "definition": "360p" } }
            ]
        });
        let s = pick_stream_at(&model, None).unwrap();
        assert_eq!(s.url, "only-one");
    }

    #[test]
    fn missing_encrypt_info_yields_empty_key() {
        let model = json!({
            "video_list": [{ "main_url": "plain", "video_meta": { "definition": "720p" } }]
        });
        let s = pick_stream_at(&model, None).unwrap();
        assert!(s.spade.is_empty(), "无 encrypt_info 时密钥应为空");
    }

    #[test]
    fn empty_list_is_none() {
        assert!(pick_stream_at(&json!({ "video_list": [] }), None).is_none());
        assert!(pick_stream_at(&json!({}), None).is_none());
    }
}

//! 清晰度择优：从 `video_list` 里挑出最该下的那一条。
//! ⚠️ 流元数据全在 `video_meta` 子对象、`definition` 带 `p` 后缀——照旧 schema 读会全取到 0；
//! 播放使用内置解码约束；下载允许按独立的编码能力选择源文件。

#[cfg(test)]
mod builtin_tests;
mod model;

pub use model::{DecoderCapability, PlayStream, StreamScore};

use std::collections::BTreeSet;

use serde_json::Value;

/// 编码器偏好：不可播 / 未识别。
const UNRANKED_CODEC: u8 = 3;
/// `bytevc2` 可播时的偏好位（次于 bytevc1；需 `cap.bytevc2`）。
const BYTEVC2_RANK: u8 = 2;

/// 编码偏好：HEVC > bytevc1 > bytevc2 > 其余。
fn codec_rank(codec: &str) -> u8 {
    if hongguo_bytevc2::is_bytevc2(codec) {
        return BYTEVC2_RANK;
    }
    match codec {
        "h265" | "h265_hvc1" | "hevc" | "hvc1" | "hev1" => 0,
        "bytevc1" => 1,
        _ => UNRANKED_CODEC,
    }
}

/// `video_meta.codec_type`，取不到按空串处理。
fn codec_of(v: &Value) -> &str {
    v.get("video_meta")
        .and_then(|m| m.get("codec_type"))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// 内置软解器能解的编码：HEVC 系（服务端称 `h265` / `bytevc1`，容器里是 `hvc1` / `hev1`）
/// 与 ByteVC2。接口的编码名和 MP4 四字码都认——在线选流拿到的是前者，本地文件探到的是后者。
/// H.264 不在其中：在线播放不回落到系统解码器。
pub fn builtin_decodable(codec: &str) -> bool {
    hongguo_bytevc2::is_bytevc2(codec)
        || matches!(
            codec,
            "h265" | "h265_hvc1" | "hevc" | "hvc1" | "hev1" | "bytevc1"
        )
}

/// 这条流是否满足当前解码能力；`bytevc2` 需内置解码器支持。
/// 没报编码器按能播处理（明确不可播才排除）。
/// [`DecoderCapability::builtin_only`] 下口径反过来——只有认得出、内置解码器能解的才算，
/// 没报编码的不赌。
pub fn is_playable(v: &Value, cap: DecoderCapability) -> bool {
    let codec = codec_of(v);
    if cap.builtin_only {
        return builtin_decodable(codec) && (cap.bytevc2 || !hongguo_bytevc2::is_bytevc2(codec));
    }
    if codec.is_empty() {
        return true;
    }
    if hongguo_bytevc2::is_bytevc2(codec) {
        return cap.bytevc2;
    }
    codec_rank(codec) < UNRANKED_CODEC
}

/// 这一集有流、但没有一条内置解码器能解时，说出服务端实际给了哪些编码（去重、保序，
/// 没标编码的记作「未标注」）。有可播的流、流表为空或不在内置模式时返回 `None`——
/// 那些情况各有各的报法，不归这里。
pub fn refused_codecs(model: &Value, cap: DecoderCapability) -> Option<String> {
    if !cap.builtin_only {
        return None;
    }
    let mut offered: Vec<&str> = Vec::new();
    for stream in model.get("video_list")?.as_array()? {
        if stream.get("main_url").and_then(Value::as_str).is_none() {
            continue;
        }
        if is_playable(stream, cap) {
            return None;
        }
        let codec = match codec_of(stream) {
            "" => "未标注",
            codec => codec,
        };
        if !offered.contains(&codec) {
            offered.push(codec);
        }
    }
    (!offered.is_empty()).then(|| offered.join("、"))
}

/// 计算一条流在指定能力模式下的分数。
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

/// 解析 `definition` 为数值：优先取 `"1080p"` 里的数字，取不到按常见档位名映射。
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

/// 从 `"1080p"` 这类字符串取数字部分，数字后必须跟可选空白 + `p`（对应 JS `/(\d+)\s*p/i`）——
/// 不带约束 `"4K"` 会被截成 4，2160 档映射走不到。
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

/// 一集实际提供的清晰度档位（= [`hongguo_common::model::VideoDefinition`]，
/// 取流与 IPC 两头都用，另立类型只会在中间多一层转换）。
pub use hongguo_common::model::VideoDefinition as Definition;

/// 列出一集**能播**的清晰度档位，按高到低（同档位多条流只给一项）；播不了的档位
/// 不进菜单，过滤必须逐集判断。
pub fn list_definitions(model: &Value, cap: DecoderCapability) -> Vec<Definition> {
    let Some(list) = model.get("video_list").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut seen: BTreeSet<Definition> = BTreeSet::new();
    for v in list {
        if v.get("main_url").and_then(Value::as_str).is_none() {
            continue;
        }
        if !is_playable(v, cap) {
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

    seen.into_iter().rev().collect()
}

/// 在候选列表里挑最优流。
fn best_in(
    list: &[Value],
    cap: DecoderCapability,
    target: Option<u32>,
) -> Option<(StreamScore, &Value)> {
    let mut best: Option<(StreamScore, &Value)> = None;
    for v in list {
        if v.get("main_url").and_then(Value::as_str).is_none() {
            continue;
        }
        if !is_playable(v, cap) {
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
    best
}

/// 从 `video_model` 里挑指定清晰度的那条流：`target` 为 `None` 挑最高档，指定档位在
/// 该档内按编码偏好 > 码率择优；播不了的流不参与挑选（缺档/私有编码档静默回退能播的最高档）；
pub fn pick_stream_at(
    model: &Value,
    target: Option<u32>,
    cap: DecoderCapability,
) -> Option<PlayStream> {
    let list = model.get("video_list")?.as_array()?;
    let chosen = best_in(list, cap, target).or_else(|| best_in(list, cap, None));
    let (score, v) = chosen?;
    Some(PlayStream {
        url: v.get("main_url")?.as_str()?.to_string(),
        file_hash: v
            .pointer("/video_meta/file_hash")
            .and_then(Value::as_str)
            .filter(|hash| !hash.is_empty())
            .map(str::to_owned),
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

    fn multi_codec_model() -> Value {
        json!({
            "video_list": [
                { "main_url": "https://cdn/720-private.mp4",
                  "video_meta": { "definition": "720p", "vwidth": 1280, "vheight": 720,
                                  "bitrate": 400000, "codec_type": "bytevc2" } },
                { "main_url": "https://cdn/720-standard.mp4",
                  "video_meta": { "definition": "720p", "vwidth": 1280, "vheight": 720,
                                  "bitrate": 400000, "codec_type": "bytevc1" } }
            ]
        })
    }

    #[test]
    fn picks_highest_definition_from_real_schema() {
        let s = pick_stream_at(&real_model(), None, DecoderCapability::full()).unwrap();
        assert_eq!(s.url, "https://cdn/1080.mp4");
        assert_eq!(s.spade, "KEY1080");
        assert_eq!(s.codec.as_deref(), Some("bytevc1"));
        assert_eq!(s.definition, 1080);
    }

    #[test]
    fn unplayable_definitions_are_kept_out_of_the_menu() {
        let defs = list_definitions(&real_model(), DecoderCapability::full());
        assert_eq!(
            defs.iter().map(|d| d.value).collect::<Vec<_>>(),
            vec![1080],
            "只有 bytevc1 那档能进菜单"
        );
    }

    #[test]
    fn bytevc2_enters_menu_when_capability_allows() {
        let cap = DecoderCapability::full().with_bytevc2(true);
        let defs = list_definitions(&real_model(), cap);
        assert_eq!(
            defs.iter().map(|d| d.value).collect::<Vec<_>>(),
            vec![1080, 720, 360],
            "有 bvc2 解码器时 bytevc2 档也进菜单"
        );
        let s = pick_stream_at(&real_model(), None, cap).unwrap();
        assert_eq!(s.codec.as_deref(), Some("bytevc1"), "同剧仍优先 bytevc1");
        let only = pick_stream_at(&multi_codec_model(), Some(720), cap).unwrap();
        assert_eq!(only.codec.as_deref(), Some("bytevc1"), "同档仍优先标准编码");
    }

    #[test]
    fn bytevc2_is_selectable_without_a_system_hevc_decoder() {
        let cap = DecoderCapability::builtin();
        let defs = list_definitions(&real_model(), cap);
        assert_eq!(
            defs.iter().map(|d| d.value).collect::<Vec<_>>(),
            vec![1080, 720, 360]
        );
        let stream = pick_stream_at(&real_model(), Some(720), cap).unwrap();
        assert_eq!(stream.definition, 720);
        assert_eq!(stream.codec.as_deref(), Some("bytevc2"));
    }

    #[test]
    fn a_standard_codec_in_the_same_definition_keeps_the_option_alive() {
        let defs = list_definitions(&multi_codec_model(), DecoderCapability::full());
        assert_eq!(defs.iter().map(|d| d.value).collect::<Vec<_>>(), vec![720]);
    }

    #[test]
    fn picks_the_playable_stream_when_a_definition_has_several_codecs() {
        let s = pick_stream_at(&multi_codec_model(), Some(720), DecoderCapability::full())
            .expect("720p 应可选");
        assert_eq!(
            s.url, "https://cdn/720-standard.mp4",
            "同档位要挑能播的那条"
        );
        assert_eq!(s.codec.as_deref(), Some("bytevc1"));
    }

    #[test]
    fn an_unplayable_definition_falls_back_to_the_best_playable_one() {
        let s = pick_stream_at(&real_model(), Some(720), DecoderCapability::full())
            .expect("应回退而不是报错");
        assert_eq!(s.url, "https://cdn/1080.mp4");
        assert_eq!(s.definition, 1080);
    }

    #[test]
    fn picks_the_requested_definition() {
        let s = pick_stream_at(&multi_codec_model(), Some(720), DecoderCapability::full())
            .expect("720p 应可选");
        assert_eq!(s.definition, 720);
    }

    #[test]
    fn unavailable_definition_falls_back_to_the_best() {
        let s = pick_stream_at(&real_model(), Some(2160), DecoderCapability::full())
            .expect("应回退而不是报错");
        assert_eq!(s.definition, 1080);
    }

    #[test]
    fn definitions_without_metadata_are_skipped() {
        let model = serde_json::json!({
            "video_list": [
                { "main_url": "a", "video_meta": { "definition": "1080p", "vwidth": 1920, "vheight": 1080 } },
                { "main_url": "b", "video_meta": {} }
            ]
        });
        let defs = list_definitions(&model, DecoderCapability::full());
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
        let s = pick_stream_at(&model, None, DecoderCapability::full()).unwrap();
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
        let s = pick_stream_at(&model, None, DecoderCapability::full()).unwrap();
        assert_eq!(s.url, "only-one");
    }

    #[test]
    fn missing_encrypt_info_yields_empty_key() {
        let model = json!({
            "video_list": [{ "main_url": "plain", "video_meta": { "definition": "720p" } }]
        });
        let s = pick_stream_at(&model, None, DecoderCapability::full()).unwrap();
        assert!(s.spade.is_empty(), "无 encrypt_info 时密钥应为空");
    }

    #[test]
    fn empty_list_is_none() {
        assert!(
            pick_stream_at(
                &json!({ "video_list": [] }),
                None,
                DecoderCapability::full()
            )
            .is_none()
        );
        assert!(pick_stream_at(&json!({}), None, DecoderCapability::full()).is_none());
    }
}

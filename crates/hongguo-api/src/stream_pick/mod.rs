//! 清晰度择优。
//!
//! 从 `video_list` 里挑出最该下的那一条。
//!
//! ⚠️ 流的元数据全在 `video_meta` 子对象里，且 `definition` 带 `p` 后缀
//!    （如 `"1080p"`）。顶层没有 `definition` / `bitrate` / 宽高——
//!    照着旧 schema 读会全部取到 0，择优退化成「永远选第一条」（360p）。
//!
//! 无 HEVC 扩展的机器走 [`DecoderCapability::NoHevc`] 模式：语义对齐官方
//! TTVideoEngine `CodecStrategy`——能力过滤与层级优先，宁选能播的 720p h264
//! 也不选必黑屏的 1080p h265，HEVC 系保留为末选（黑屏后由转码兜底接住）。

mod model;

pub use model::{DecoderCapability, PlayStream, StreamScore};

use std::collections::BTreeSet;

use serde_json::Value;

/// 编码器偏好排序里「排不进前两档」的编码器。
const UNRANKED_CODEC: u8 = 2;

/// 无 HEVC 模式的编码层级（官方 FALLBACK 序列的桌面版）。
const NO_HEVC_H264: u8 = 0;
const NO_HEVC_UNKNOWN: u8 = 1;
const NO_HEVC_HEVC: u8 = 2;

/// 编码器偏好排序。
///
/// 平台优先给 `h265` / `h264`，其次 `bytevc1`，其余（含 `bytevc2`）最后。
/// 同一清晰度下优先选标准编码器——ffmpeg 与系统播放器的兼容性差异很大。
fn codec_rank(codec: &str) -> u8 {
    match codec {
        "h265" | "h264" | "h265_hvc1" | "hevc" | "hvc1" | "avc1" => 0,
        "bytevc1" => 1,
        _ => UNRANKED_CODEC,
    }
}

/// 是否为 H.264 系（无 HEVC 机器上唯一必然可解的编码）。
fn is_h264(codec: &str) -> bool {
    matches!(codec, "h264" | "avc1")
}

/// 无 HEVC 模式的编码层级。
fn codec_tier_no_hevc(codec: &str) -> u8 {
    if is_h264(codec) {
        NO_HEVC_H264
    } else if codec.is_empty() {
        NO_HEVC_UNKNOWN
    } else {
        NO_HEVC_HEVC
    }
}

/// `video_meta.codec_type`，取不到按空串处理。
fn codec_of(v: &Value) -> &str {
    v.get("video_meta")
        .and_then(|m| m.get("codec_type"))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// 这条流能不能直接喂给播放器。
///
/// `bytevc2`（ByteVC 2）是字节跳动的私有编码：它的参数集装在私有 `bv2C` box 里
/// 而不是标准 `hvcC`，而且码流里**没有** in-band 的 VPS/SPS/PPS。Chromium /
/// WebView2 不认 `bv2C`，也**不会退软解**——容器照样解析得出时长、样本照样缓冲
/// 到位，唯独送进解码器的码流缺参数集，硬件解码器直接
/// `PIPELINE_ERROR_DECODE`（WebView 表现是「视频处理失败」黑屏，MediaError code=3）。
///
/// 平台是**按集**分配编码器的，所以这不是「低清晰度一律不能切」：同一部剧里
/// 14/16 集的 720P 给的是标准 `bytevc1`（照常可切），而 12/13 集的 720P 只给
/// `bytevc2`。判据必须落在每条流自己的 `codec_type` 上。
///
/// **没报编码器时按能播处理**。判据是「明确不可播才排除」而不是「不在白名单就
/// 排除」：一集里所有流都缺 `codec_type` 时，严格白名单会一档都挑不出来，
/// 表现从「少一个清晰度选项」升级成「整集播不了」。
pub fn is_playable(v: &Value) -> bool {
    let codec = codec_of(v);
    codec.is_empty() || codec_rank(codec) < UNRANKED_CODEC
}

/// 计算一条流在指定能力模式下的分数。
pub fn stream_score_with(v: &Value, cap: DecoderCapability) -> StreamScore {
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
        no_hevc: cap == DecoderCapability::NoHevc,
        codec_tier: match cap {
            DecoderCapability::Full => 0,
            DecoderCapability::NoHevc => codec_tier_no_hevc(codec),
        },
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
/// 就是 [`hongguo_common::model::VideoDefinition`]：取流与 IPC 两头都要用，
/// 另立一个类型只会在中间多一层转换。
pub use hongguo_common::model::VideoDefinition as Definition;

/// 列出一集**能播**的清晰度档位，按高到低。
///
/// 同一档位通常有多条流（h265 / bytevc1 / bytevc2 各一条），菜单按档位
/// 给一项即可，具体选哪条编码仍由 [`pick_stream_at`] 的择优规则决定。
///
/// 播不了的档位不进菜单：列出来让用户点，点了就是一次黑屏。平台是**按集**
/// 分配编码器的——同一部剧里 14 集的 720P 给标准 `bytevc1` 能切，第 13 集的
/// 720P 只给私有 `bytevc2`，所以这个过滤必须逐集判断，不能按「分辨率越高越
/// 标准」这种想当然的规律写死。
///
/// `NoHevc` 模式下，最优流仍是 HEVC 系的档位同样不进菜单——无扩展机器上
/// 点了必黑屏，末选 HEVC 只在整集别无选择时由 [`pick_stream_at`] 兜底取用。
pub fn list_definitions(model: &Value, cap: DecoderCapability) -> Vec<Definition> {
    let Some(list) = model.get("video_list").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut seen: BTreeSet<Definition> = BTreeSet::new();
    for v in list {
        if v.get("main_url").and_then(Value::as_str).is_none() {
            continue;
        }
        if !is_playable(v) {
            continue;
        }
        let score = stream_score_with(v, cap);
        if cap == DecoderCapability::NoHevc && score.codec_tier >= NO_HEVC_HEVC {
            continue;
        }
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

/// 在候选列表里挑最优流。`decodable_only` 为 true 时只看「必然/大概率可解」
/// 的层级（h264 / 未知编码），供 NoHevc 模式的分轮择优。
fn best_in(
    list: &[Value],
    cap: DecoderCapability,
    target: Option<u32>,
    decodable_only: bool,
) -> Option<(StreamScore, &Value)> {
    let mut best: Option<(StreamScore, &Value)> = None;
    for v in list {
        if v.get("main_url").and_then(Value::as_str).is_none() {
            continue;
        }
        if !is_playable(v) {
            continue;
        }
        let score = stream_score_with(v, cap);
        if decodable_only && score.codec_tier >= NO_HEVC_HEVC {
            continue;
        }
        if target.is_some_and(|t| score.def_num != t) {
            continue;
        }
        if best.as_ref().is_none_or(|(b, _)| score > *b) {
            best = Some((score, v));
        }
    }
    best
}

/// 从 `video_model` 里挑指定清晰度的那条流。
///
/// `target` 为 `None` 时挑最高档（沿用 [`StreamScore`] 的完整择优规则）；
/// 指定档位时在该档位内择优——同一档位的多条流编码器不同，仍要按
/// 编码器偏好 > 码率 挑出最好的那条。
///
/// 播不了的流一律不参与挑选（见 [`is_playable`]），于是「切到平台没给的那档」
/// 与「切到平台只给了私有编码的那档」走同一条回退路径：静默退回能播的最高档。
///
/// `NoHevc` 模式分轮择优（官方维度序列语义）：先在**可解层级**（h264/未知）
/// 里按目标档位、再全集挑选；都落空才把 HEVC 系当末选——整集只有 HEVC 时
/// 宁可取流后靠黑屏签名触发转码兜底，也不让取流直接失败。
pub fn pick_stream_at(
    model: &Value,
    target: Option<u32>,
    cap: DecoderCapability,
) -> Option<PlayStream> {
    let list = model.get("video_list")?.as_array()?;
    let chosen = match cap {
        DecoderCapability::Full => {
            best_in(list, cap, target, false).or_else(|| best_in(list, cap, None, false))
        }
        DecoderCapability::NoHevc => best_in(list, cap, target, true)
            .or_else(|| best_in(list, cap, None, true))
            .or_else(|| best_in(list, cap, target, false))
            .or_else(|| best_in(list, cap, None, false)),
    };
    let (score, v) = chosen?;
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

    /// 同一档位有多条流时，优先取能播的那条。
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
        let s = pick_stream_at(&real_model(), None, DecoderCapability::Full).unwrap();
        assert_eq!(s.url, "https://cdn/1080.mp4");
        assert_eq!(s.spade, "KEY1080");
        assert_eq!(s.codec.as_deref(), Some("bytevc1"));
        assert_eq!(s.definition, 1080);
    }

    #[test]
    fn unplayable_definitions_are_kept_out_of_the_menu() {
        // 实测：12/13 集的 720P 只有私有 bytevc2，点下去必定黑屏，
        // 所以菜单里不能出现这一档。
        let defs = list_definitions(&real_model(), DecoderCapability::Full);
        assert_eq!(
            defs.iter().map(|d| d.value).collect::<Vec<_>>(),
            vec![1080],
            "只有 bytevc1 那档能进菜单"
        );
    }

    #[test]
    fn a_standard_codec_in_the_same_definition_keeps_the_option_alive() {
        // 平台按集分配编码器：14/16 集的 720P 给的是 bytevc1，菜单照常显示 720P
        let defs = list_definitions(&multi_codec_model(), DecoderCapability::Full);
        assert_eq!(defs.iter().map(|d| d.value).collect::<Vec<_>>(), vec![720]);
    }

    #[test]
    fn picks_the_playable_stream_when_a_definition_has_several_codecs() {
        let s = pick_stream_at(&multi_codec_model(), Some(720), DecoderCapability::Full)
            .expect("720p 应可选");
        assert_eq!(
            s.url, "https://cdn/720-standard.mp4",
            "同档位要挑能播的那条"
        );
        assert_eq!(s.codec.as_deref(), Some("bytevc1"));
    }

    #[test]
    fn an_unplayable_definition_falls_back_to_the_best_playable_one() {
        // 请求 720p，但这一集只给了私有 bytevc2 的 720p → 静默回退到 1080p，
        // 而不是把一条播不了的流喂给解码器
        let s = pick_stream_at(&real_model(), Some(720), DecoderCapability::Full)
            .expect("应回退而不是报错");
        assert_eq!(s.url, "https://cdn/1080.mp4");
        assert_eq!(s.definition, 1080);
    }

    #[test]
    fn picks_the_requested_definition() {
        let s = pick_stream_at(&multi_codec_model(), Some(720), DecoderCapability::Full)
            .expect("720p 应可选");
        assert_eq!(s.definition, 720);
    }

    #[test]
    fn unavailable_definition_falls_back_to_the_best() {
        // 平台没给 4K 时静默回退到最高档，而不是让播放直接失败
        let s = pick_stream_at(&real_model(), Some(2160), DecoderCapability::Full)
            .expect("应回退而不是报错");
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
        let defs = list_definitions(&model, DecoderCapability::Full);
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
            .map(|v| stream_score_with(v, DecoderCapability::Full))
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
        assert_eq!(
            stream_score_with(&v, DecoderCapability::Full).bitrate,
            88888
        );
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
        let s = pick_stream_at(&model, None, DecoderCapability::Full).unwrap();
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
        let s = pick_stream_at(&model, None, DecoderCapability::Full).unwrap();
        assert_eq!(s.url, "only-one");
    }

    #[test]
    fn missing_encrypt_info_yields_empty_key() {
        let model = json!({
            "video_list": [{ "main_url": "plain", "video_meta": { "definition": "720p" } }]
        });
        let s = pick_stream_at(&model, None, DecoderCapability::Full).unwrap();
        assert!(s.spade.is_empty(), "无 encrypt_info 时密钥应为空");
    }

    #[test]
    fn empty_list_is_none() {
        assert!(
            pick_stream_at(&json!({ "video_list": [] }), None, DecoderCapability::Full).is_none()
        );
        assert!(pick_stream_at(&json!({}), None, DecoderCapability::Full).is_none());
    }

    // ——————— NoHevc（无 HEVC 扩展机器）矩阵 ———————
    // 语义对齐官方 CodecStrategy：能力决定「谁参赛、怎么排」。

    /// 核心场景：1080p 只有 h265（必黑屏），720p 有 h264（必然可解）。
    fn hevc_vs_h264_model() -> Value {
        json!({
            "video_list": [
                { "main_url": "https://cdn/1080-hevc.mp4",
                  "video_meta": { "definition": "1080p", "vwidth": 1080, "vheight": 1920,
                                  "bitrate": 503006, "codec_type": "h265" } },
                { "main_url": "https://cdn/720-h264.mp4",
                  "video_meta": { "definition": "720p", "vwidth": 720, "vheight": 1280,
                                  "bitrate": 400000, "codec_type": "h264" } }
            ]
        })
    }

    #[test]
    fn no_hevc_machine_drops_to_the_playable_definition() {
        // 宁选能播的 720p h264，也不选必黑屏的 1080p h265
        let s = pick_stream_at(&hevc_vs_h264_model(), None, DecoderCapability::NoHevc).unwrap();
        assert_eq!(s.url, "https://cdn/720-h264.mp4");
        assert_eq!(s.codec.as_deref(), Some("h264"));
        assert_eq!(s.definition, 720);
        // 有能力时同一集仍是清晰的 1080p
        let s = pick_stream_at(&hevc_vs_h264_model(), None, DecoderCapability::Full).unwrap();
        assert_eq!(s.url, "https://cdn/1080-hevc.mp4");
    }

    #[test]
    fn no_hevc_machine_keeps_hevc_only_gears_out_of_the_menu() {
        let defs = list_definitions(&hevc_vs_h264_model(), DecoderCapability::NoHevc);
        assert_eq!(
            defs.iter().map(|d| d.value).collect::<Vec<_>>(),
            vec![720],
            "HEVC-only 档在无扩展机器上点了必黑屏，不能进菜单"
        );
    }

    #[test]
    fn no_hevc_same_definition_prefers_h264_over_higher_bitrate_hevc() {
        // 同档位 h265 码率更低（HEVC 压缩优势），NoHevc 下也让位给 h264
        let model = json!({
            "video_list": [
                { "main_url": "hevc", "video_meta": { "definition": "720p", "vwidth": 720,
                    "vheight": 1280, "bitrate": 400000, "codec_type": "h265" } },
                { "main_url": "h264", "video_meta": { "definition": "720p", "vwidth": 720,
                    "vheight": 1280, "bitrate": 600000, "codec_type": "h264" } }
            ]
        });
        let s = pick_stream_at(&model, Some(720), DecoderCapability::NoHevc).unwrap();
        assert_eq!(s.url, "h264");
    }

    #[test]
    fn no_hevc_last_resort_skips_bytevc2_but_keeps_standard_hevc() {
        // bytevc2 是私有编码，两模式都排除；bytevc1（标准参数集 HEVC）作为
        // 末选仍可取——黑屏签名交给转码兜底，取流直接失败反而断掉兜底链
        let s = pick_stream_at(&multi_codec_model(), None, DecoderCapability::NoHevc).unwrap();
        assert_eq!(s.codec.as_deref(), Some("bytevc1"));
    }

    #[test]
    fn no_hevc_whole_hevc_episode_still_yields_a_stream_as_last_resort() {
        // 整集只有 HEVC：仍取流（末选），黑屏后由转码兜底接住——
        // 取流直接失败会让兜底链无从触发
        let s = pick_stream_at(&real_model(), None, DecoderCapability::NoHevc).unwrap();
        assert_eq!(s.definition, 1080, "末选取 HEVC 里最高的 1080p bytevc1");
    }

    #[test]
    fn no_hevc_unknown_codec_stays_as_the_last_resort() {
        // 全集缺 codec_type：不能把整集选没了（「整集播不了」底线）
        let model = json!({
            "video_list": [
                { "main_url": "mystery", "video_meta": { "definition": "720p",
                    "vwidth": 720, "vheight": 1280, "bitrate": 300000 } }
            ]
        });
        let s = pick_stream_at(&model, None, DecoderCapability::NoHevc).unwrap();
        assert_eq!(s.url, "mystery");
    }

    #[test]
    fn no_hevc_prefers_h264_over_unknown_at_same_definition() {
        let model = json!({
            "video_list": [
                { "main_url": "mystery", "video_meta": { "definition": "720p",
                    "vwidth": 720, "vheight": 1280, "bitrate": 500000 } },
                { "main_url": "h264", "video_meta": { "definition": "720p",
                    "vwidth": 720, "vheight": 1280, "bitrate": 400000, "codec_type": "h264" } }
            ]
        });
        let s = pick_stream_at(&model, Some(720), DecoderCapability::NoHevc).unwrap();
        assert_eq!(s.url, "h264", "同档位确定的 h264 优先于未知编码");
    }

    #[test]
    fn no_hevc_pinned_hevc_only_definition_falls_back_silently() {
        // pin 了只有 bytevc2 的 720p → 静默回退到 bytevc1 的 1080p；
        // 与 Full 模式「私有编码档静默回退」同语义
        let s = pick_stream_at(&real_model(), Some(720), DecoderCapability::NoHevc).unwrap();
        assert_eq!(s.definition, 1080);
    }

    #[test]
    fn no_hevc_unknown_gear_stays_in_the_menu_as_risky_but_playable() {
        // 未知编码的档位保留在菜单：可能能播（黑屏有兜底），不该直接藏掉
        let model = json!({
            "video_list": [
                { "main_url": "mystery", "video_meta": { "definition": "1080p",
                    "vwidth": 1080, "vheight": 1920, "bitrate": 500000 } },
                { "main_url": "h264", "video_meta": { "definition": "720p",
                    "vwidth": 720, "vheight": 1280, "bitrate": 400000, "codec_type": "h264" } }
            ]
        });
        let defs = list_definitions(&model, DecoderCapability::NoHevc);
        assert_eq!(
            defs.iter().map(|d| d.value).collect::<Vec<_>>(),
            vec![1080, 720]
        );
    }
}

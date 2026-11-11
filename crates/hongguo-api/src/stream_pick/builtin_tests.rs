//! 内置软解模式（[`DecoderCapability::builtin`]）的选流口径。
//! 下面的流表都是为边界构造的，不是抓包——真实服务目前只观测到 `bytevc1` 与 `bytevc2`。

use serde_json::{Value, json};

use super::*;

fn stream(url: &str, definition: &str, codec: Option<&str>) -> Value {
    let mut meta = json!({
        "definition": definition, "vwidth": 720, "vheight": 1280, "bitrate": 400_000
    });
    if let Some(codec) = codec {
        meta["codec_type"] = json!(codec);
    }
    json!({ "main_url": url, "video_meta": meta })
}

fn model(streams: Vec<Value>) -> Value {
    json!({ "video_list": streams })
}

fn menu(model: &Value, cap: DecoderCapability) -> Vec<u32> {
    list_definitions(model, cap)
        .iter()
        .map(|definition| definition.value)
        .collect()
}

#[test]
fn builtin_names_cover_wire_names_and_fourccs_but_not_h264() {
    for codec in [
        "h265",
        "h265_hvc1",
        "hevc",
        "hvc1",
        "hev1",
        "bytevc1",
        "bytevc2",
        "bvc2",
    ] {
        assert!(builtin_decodable(codec), "{codec} 应由内置解码器解");
    }
    for codec in ["h264", "avc1", "av1", "vp9", "encv", ""] {
        assert!(!builtin_decodable(codec), "{codec:?} 不在内置解码器范围内");
    }
}

#[test]
fn builtin_offers_every_hevc_and_bytevc2_gear_regardless_of_system_hevc() {
    let model = model(vec![
        stream("360", "360p", Some("bytevc2")),
        stream("720", "720p", Some("bytevc2")),
        stream("1080", "1080p", Some("bytevc1")),
    ]);
    let cap = DecoderCapability::builtin();
    assert_eq!(menu(&model, cap), [1080, 720, 360]);
    assert_eq!(
        pick_stream_at(&model, None, cap).expect("最高档").url,
        "1080"
    );
    assert_eq!(
        pick_stream_at(&model, Some(720), cap).expect("指定档").url,
        "720"
    );
}

#[test]
fn builtin_prefers_hevc_over_bytevc2_within_one_gear() {
    let model = model(vec![
        stream("vvc", "720p", Some("bytevc2")),
        stream("hevc", "720p", Some("bytevc1")),
    ]);
    let picked = pick_stream_at(&model, Some(720), DecoderCapability::builtin()).expect("720");
    assert_eq!(picked.url, "hevc", "同档位 HEVC 软解更省，优先于 ByteVC2");
}

#[test]
fn builtin_never_picks_h264_even_when_it_is_the_highest_gear() {
    let model = model(vec![
        stream("h264-1080", "1080p", Some("h264")),
        stream("hevc-720", "720p", Some("bytevc1")),
    ]);
    let cap = DecoderCapability::builtin();
    assert_eq!(menu(&model, cap), [720], "只有 H.264 的档位不进菜单");
    assert_eq!(
        pick_stream_at(&model, None, cap).expect("最高可播档").url,
        "hevc-720"
    );
    let pinned = pick_stream_at(&model, Some(1080), cap).expect("回退到可播档");
    assert_eq!(
        pinned.definition, 720,
        "点名 1080 时如实返回实际选中的 720，而不是给 H.264"
    );
    assert_eq!(refused_codecs(&model, cap), None, "有可播的流就不算拒绝");
}

#[test]
fn builtin_refuses_an_h264_only_episode_and_names_the_codec() {
    let model = model(vec![
        stream("a", "720p", Some("h264")),
        stream("b", "1080p", Some("h264")),
    ]);
    let cap = DecoderCapability::builtin();
    assert!(pick_stream_at(&model, None, cap).is_none());
    assert!(menu(&model, cap).is_empty());
    assert_eq!(refused_codecs(&model, cap).as_deref(), Some("h264"));
}

#[test]
fn builtin_does_not_gamble_on_missing_or_unknown_codecs() {
    let model = model(vec![
        stream("unlabelled", "1080p", None),
        stream("av1", "720p", Some("av1")),
        stream("blank", "540p", Some("")),
    ]);
    let cap = DecoderCapability::builtin();
    assert!(
        pick_stream_at(&model, None, cap).is_none(),
        "没标编码的流不能按能播处理"
    );
    assert_eq!(refused_codecs(&model, cap).as_deref(), Some("未标注、av1"));
}

#[test]
fn builtin_without_the_bytevc2_decoder_refuses_a_bytevc2_only_episode() {
    let model = model(vec![stream("vvc", "720p", Some("bytevc2"))]);
    let cap = DecoderCapability::builtin().with_bytevc2(false);
    assert!(pick_stream_at(&model, None, cap).is_none());
    assert_eq!(refused_codecs(&model, cap).as_deref(), Some("bytevc2"));
}

#[test]
fn refusal_is_only_reported_for_builtin_mode_and_real_streams() {
    let h264_only = model(vec![stream("a", "720p", Some("h264"))]);
    assert_eq!(refused_codecs(&h264_only, DecoderCapability::full()), None);
    assert_eq!(
        refused_codecs(&model(vec![]), DecoderCapability::builtin()),
        None
    );
    // 没有地址的条目不是流，不该被算成「给了 h264」。
    let no_url = json!({ "video_list": [{ "video_meta": { "codec_type": "h264" } }] });
    assert_eq!(refused_codecs(&no_url, DecoderCapability::builtin()), None);
}

#[test]
fn download_selection_is_untouched_and_still_takes_h264() {
    let model = model(vec![stream("h264", "1080p", Some("h264"))]);
    let picked = pick_stream_at(&model, None, DecoderCapability::full()).expect("下载口径不变");
    assert_eq!(picked.url, "h264");
}

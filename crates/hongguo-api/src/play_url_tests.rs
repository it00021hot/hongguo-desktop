use serde_json::json;

use super::*;
use crate::stream_pick::DecoderCapability;

#[test]
fn registry_roundtrip_and_zero_skip() {
    register_content_type("v1", 1004);
    register_content_type("", 1);
    register_content_type("v-zero", 0);
    assert_eq!(content_type_of("v1"), Some(1004));
    assert_eq!(content_type_of(""), None);
    assert_eq!(content_type_of("v-zero"), None);
    assert_eq!(content_type_of("未注册"), None);
    register_content_type("v1", 1);
    assert_eq!(content_type_of("v1"), Some(1));
}

#[test]
fn clear_batch_cache_drops_stale_prefetch() {
    batch_cache().lock().insert(
        "vid-old-device".into(),
        (Instant::now(), Arc::new(Value::Null)),
    );
    assert!(batch_cache().lock().contains_key("vid-old-device"));
    clear_batch_cache();
    assert!(batch_cache().lock().is_empty(), "换档后旧设备预取必须清空");
}

#[test]
fn model_error_envelope_surfaces_with_server_message() {
    let v: Value = serde_json::from_str(
        r#"{"Code":110001,"Message":"未知异常","BaseResp":{"StatusCode":110001,"StatusMessage":"unknown error"}}"#,
    )
    .unwrap();
    let err = check_model_code(&v).unwrap_err().to_string();
    assert!(err.contains("110001"), "{err}");
    assert!(err.contains("unknown error"), "{err}");
}

#[test]
fn model_success_envelope_without_code_passes() {
    check_model_code(&serde_json::json!({ "data": {} })).unwrap();
    check_model_code(&serde_json::json!({ "Code": 0, "data": {} })).unwrap();
}

#[test]
fn model_authentication_rejection_is_not_a_media_error() {
    let error = parse_model_response(br#"{"code":101108,"message":"USER_NOT_LOGIN"}"#).unwrap_err();
    assert!(matches!(error, AppError::Auth(_)));
    assert!(error.to_string().contains("101108"));
    assert!(error.to_string().contains("USER_NOT_LOGIN"));
}

#[test]
fn unknown_model_business_code_keeps_its_existing_classification() {
    let error = parse_model_response(br#"{"code":100001,"message":"unconfirmed"}"#).unwrap_err();
    assert!(matches!(error, AppError::Media(_)));
    assert!(error.to_string().contains("100001"));
}

fn model(codec: &str) -> String {
    json!({"video_list": [{
        "main_url": "https://cdn.example/video.mp4",
        "video_meta": {"definition": "1080p", "codec_type": codec},
        "encrypt_info": {"spade_a": "test-key"}
    }]})
    .to_string()
}

fn response(data: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"code": 0, "BaseResp": {"StatusCode": 0}, "data": data})).unwrap()
}

#[test]
fn batch_model_cache_selects_the_requested_definition_and_can_return_to_auto() {
    let vid = "cached-multiple-definitions";
    let video_model = json!({"video_list": [
        {"main_url": "https://cdn.example/1080.mp4", "video_meta": {"definition": "1080p", "codec_type": "hevc"}},
        {"main_url": "https://cdn.example/720.mp4", "video_meta": {"definition": "720p", "codec_type": "hevc"}}
    ]})
    .to_string();
    let cap = DecoderCapability::full();
    let bytes = response(json!({vid: {"video_model": video_model}}));
    let value = parse_model_response(&bytes).unwrap();
    let models = batch_models(value.as_ref(), &[vid.to_string(), "missing".into()], cap);
    assert_eq!(models.len(), 1);
    store_models(models);
    assert_eq!(
        cached_play(vid, Some(720), cap).unwrap().url,
        "https://cdn.example/720.mp4"
    );
    assert_eq!(
        cached_play(vid, None, cap).unwrap().url,
        "https://cdn.example/1080.mp4"
    );
}

#[test]
fn single_accepts_both_model_response_shapes() {
    for data in [
        json!({"video_model": model("hevc")}),
        json!({"single-hit": {"video_model": model("hevc")}}),
    ] {
        let (_, play) = select_single_response(
            &response(data),
            "single-hit",
            1004,
            None,
            DecoderCapability::full(),
        )
        .unwrap();
        assert_eq!(play.codec, "hevc");
        assert_eq!(play.definition, 1080);
        assert!(play.encrypted);
    }
}

#[test]
fn single_distinguishes_empty_unplayable_and_server_errors() {
    let cap = DecoderCapability::full();
    let select = |bytes: &[u8]| select_single_response(bytes, "single-miss", 1004, None, cap);
    assert!(matches!(select(&[]), Err(AppError::EmptyResponse(_))));
    for data in [
        json!({}),
        json!({"video_model": model("bytevc2")}),
        json!({"single-miss": {"video_model": model("bytevc2")}}),
    ] {
        assert!(matches!(
            select(&response(data)),
            Err(AppError::StreamMiss(_))
        ));
    }
    let error = select(br#"{"Code":110001,"Message":"test failure"}"#).unwrap_err();
    assert!(matches!(error, AppError::StreamMiss(_)));
    assert!(error.to_string().contains("test failure"));
}

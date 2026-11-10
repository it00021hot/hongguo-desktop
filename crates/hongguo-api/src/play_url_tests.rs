use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;

use serde_json::json;

use super::*;
use crate::client::ApiEnv;
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

struct Request {
    path: String,
    body: Value,
}

fn mock(responses: Vec<Vec<u8>>) -> (String, JoinHandle<Vec<Request>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let thread = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for response in responses {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "mock 未收到预期请求");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("accept: {e}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut raw = Vec::new();
            let (header_end, len) = loop {
                let mut chunk = [0; 4096];
                let n = stream.read(&mut chunk).unwrap();
                assert_ne!(n, 0);
                raw.extend_from_slice(&chunk[..n]);
                if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&raw[..end]);
                    let len: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap();
                    break (end + 4, len);
                }
            };
            while raw.len() < header_end + len {
                let mut chunk = [0; 4096];
                let n = stream.read(&mut chunk).unwrap();
                assert_ne!(n, 0);
                raw.extend_from_slice(&chunk[..n]);
            }
            let path = String::from_utf8_lossy(&raw[..header_end])
                .lines()
                .next()
                .unwrap()
                .to_string();
            let body = flate2::read::GzDecoder::new(&raw[header_end..header_end + len]);
            requests.push(Request {
                path,
                body: serde_json::from_reader(body).unwrap(),
            });
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.len()).unwrap();
            stream.write_all(&response).unwrap();
        }
        requests
    });
    (origin, thread)
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

fn error_envelope() -> Vec<u8> {
    serde_json::to_vec(&json!({"Code": 110001, "Message": "test failure"})).unwrap()
}

fn local_origins(origin: &str) -> PlayOrigins<'_> {
    PlayOrigins {
        player: origin,
        saas: origin,
        light: origin,
    }
}

#[tokio::test]
async fn cached_model_selects_the_requested_definition_and_can_return_to_auto() {
    let vid = "cached-multiple-definitions";
    let video_model = json!({"video_list": [
        {"main_url": "https://cdn.example/1080.mp4", "video_meta": {"definition": "1080p", "codec_type": "h264"}},
        {"main_url": "https://cdn.example/720.mp4", "video_meta": {"definition": "720p", "codec_type": "h264"}}
    ]})
    .to_string();
    let (origin, server) = mock(vec![response(json!({vid: {"video_model": video_model}}))]);
    let group = BTreeMap::from([("1004".to_string(), vec![vid.to_string()])]);
    let cap = DecoderCapability::full();
    let env = ApiEnv::anonymous(Default::default());
    assert_eq!(
        fetch_play_batch_at(&group, cap, &env, local_origins(&origin))
            .await
            .unwrap(),
        1
    );
    assert_eq!(server.join().unwrap().len(), 1);

    let selected = fetch_play_url(vid, Some(720), cap, &env).await.unwrap();
    let automatic = fetch_play_url(vid, None, cap, &env).await.unwrap();
    assert_eq!(selected.url, "https://cdn.example/720.mp4");
    assert_eq!(automatic.url, "https://cdn.example/1080.mp4");
}

#[tokio::test]
async fn single_private_codec_falls_back_for_both_response_shapes() {
    for data in [
        json!({"video_model": model("bytevc2")}),
        json!({"single-private": {"video_model": model("bytevc2")}}),
    ] {
        let (origin, server) = mock(vec![
            response(data),
            response(json!({"single-private": {"video_model": model("bytevc1")}})),
        ]);
        let env = ApiEnv::anonymous(Default::default());
        let play = fetch_single_at(
            "single-private",
            1004,
            None,
            DecoderCapability::full(),
            &env,
            local_origins(&origin),
        )
        .await
        .unwrap()
        .into_play()
        .unwrap();
        assert_eq!(play.codec, "bytevc1");
        assert_eq!(play.definition, 1080);
        assert!(play.encrypted);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].path.contains(crate::params::SINGLE_MODEL_PATH));
        assert_eq!(
            requests[0].body["biz_param"]["device_level"], 3,
            "单集 device_level=High"
        );
        assert_eq!(requests[0].body["content_type"], 1004);
        assert_eq!(requests[0].body["video_id"], "single-private");
        assert_eq!(
            requests[1].body["mixed_video_id_map"],
            json!({"1004": ["single-private"]})
        );
        assert!(requests[1].path.contains(crate::params::MODEL_LIGHT_PATH));
    }
}

#[tokio::test]
async fn single_playable_model_does_not_call_fallbacks_or_duplicate_query() {
    let (origin, server) = mock(vec![response(json!({"video_model": model("h264")}))]);
    let env = ApiEnv::anonymous(Default::default());
    let play = fetch_single_at(
        "single-primary",
        1,
        None,
        DecoderCapability::no_hevc(),
        &env,
        local_origins(&origin),
    )
    .await
    .unwrap()
    .into_play()
    .unwrap();
    assert_eq!(play.codec, "h264");
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].path.contains(crate::params::SINGLE_MODEL_PATH));
    assert_eq!(requests[0].body["biz_param"]["device_level"], 3);
    assert_eq!(requests[0].path.matches("aid=").count(), 1);
    assert_eq!(requests[0].path.matches("cdid=").count(), 1);
}

#[tokio::test]
async fn single_empty_or_missing_model_falls_back_to_saas() {
    for single in [Vec::new(), response(json!({}))] {
        let (origin, server) = mock(vec![
            single,
            response(json!({"single-empty": {"video_model": model("h264")}})),
        ]);
        let env = ApiEnv::anonymous(Default::default());
        let play = fetch_single_at(
            "single-empty",
            1,
            None,
            DecoderCapability::no_hevc(),
            &env,
            local_origins(&origin),
        )
        .await
        .unwrap()
        .into_play()
        .unwrap();
        assert_eq!(play.codec, "h264");
        assert_eq!(server.join().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn single_error_envelope_falls_back_to_saas() {
    let (origin, server) = mock(vec![
        error_envelope(),
        response(json!({"single-envelope": {"video_model": model("h264")}})),
    ]);
    let env = ApiEnv::anonymous(Default::default());
    let play = fetch_single_at(
        "single-envelope",
        1,
        None,
        DecoderCapability::no_hevc(),
        &env,
        local_origins(&origin),
    )
    .await
    .unwrap()
    .into_play()
    .unwrap();
    assert_eq!(play.codec, "h264");
    assert_eq!(server.join().unwrap().len(), 2);
}

#[tokio::test]
async fn batch_only_falls_back_for_unplayable_or_missing_entries() {
    let (origin, server) = mock(vec![
        response(
            json!({"batch-ok": {"video_model": model("h264")}, "batch-private": {"video_model": model("bytevc2")}}),
        ),
        response(
            json!({"batch-private": {"video_model": model("bytevc1")}, "batch-missing": {"video_model": model("h264")}}),
        ),
    ]);
    let map = BTreeMap::from([
        ("1".into(), vec!["batch-ok".into()]),
        (
            "1004".into(),
            vec!["batch-private".into(), "batch-missing".into()],
        ),
    ]);
    let env = ApiEnv::anonymous(Default::default());
    assert_eq!(
        fetch_play_batch_at(
            &map,
            DecoderCapability::full(),
            &env,
            local_origins(&origin)
        )
        .await
        .unwrap(),
        3
    );
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].body["mixed_video_id_map"],
        json!({"1004": ["batch-private", "batch-missing"]})
    );
}

#[tokio::test]
async fn batch_preserves_primary_hits_if_fallback_fails() {
    let (origin, server) = mock(vec![
        response(json!({"batch-keep": {"video_model": model("h264")}})),
        error_envelope(),
    ]);
    let map = BTreeMap::from([("1".into(), vec!["batch-keep".into(), "batch-failed".into()])]);
    let env = ApiEnv::anonymous(Default::default());
    assert_eq!(
        fetch_play_batch_at(
            &map,
            DecoderCapability::full(),
            &env,
            local_origins(&origin)
        )
        .await
        .unwrap(),
        1
    );
    assert_eq!(server.join().unwrap().len(), 2);
}

#[tokio::test]
async fn batch_primary_error_envelope_falls_back_all() {
    let (origin, server) = mock(vec![
        error_envelope(),
        response(json!({"batch-envelope": {"video_model": model("h264")}})),
    ]);
    let map = BTreeMap::from([("1".into(), vec!["batch-envelope".into()])]);
    let env = ApiEnv::anonymous(Default::default());
    assert_eq!(
        fetch_play_batch_at(
            &map,
            DecoderCapability::full(),
            &env,
            local_origins(&origin)
        )
        .await
        .unwrap(),
        1
    );
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].path.contains(crate::params::MODEL_LIGHT_PATH));
    assert!(requests[0].body.get("dr_scene").is_none());
    assert_eq!(requests[0].body["biz_param"]["use_server_dns"], true);
    assert_eq!(
        requests[0].body["biz_param"]["use_server_dns_scene"],
        "cold_start_stage"
    );
    assert!(requests[1].path.contains(crate::params::PLAYER_MODEL_PATH));
}

#[tokio::test]
async fn single_unplayable_responses_return_miss() {
    let (origin, server) = mock(vec![
        response(json!({"video_model": model("bytevc2")})),
        response(json!({"single-miss": {"video_model": model("bytevc2")}})),
        response(json!({"video_model": model("bytevc2")})),
    ]);
    let env = ApiEnv::anonymous(Default::default());
    assert!(matches!(
        fetch_single_at(
            "single-miss",
            1004,
            None,
            DecoderCapability::full(),
            &env,
            local_origins(&origin)
        )
        .await
        .unwrap(),
        SingleFetch::NoPlayable
    ));
    assert_eq!(server.join().unwrap().len(), 3);
}

#[tokio::test]
async fn single_all_empty_bodies_are_silent_empty() {
    // 单集 + saas + light 三跳全空
    let (origin, server) = mock(vec![Vec::new(), Vec::new(), Vec::new()]);
    let env = ApiEnv::anonymous(Default::default());
    assert!(matches!(
        fetch_single_at(
            "single-silent",
            1004,
            None,
            DecoderCapability::full(),
            &env,
            local_origins(&origin)
        )
        .await
        .unwrap(),
        SingleFetch::SilentEmpty
    ));
    assert_eq!(server.join().unwrap().len(), 3);
}

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;

use serde_json::json;

use super::*;
use crate::client::ApiEnv;
use crate::stream_pick::DecoderCapability;

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
                        assert!(Instant::now() < deadline, "备用通道未收到预期请求");
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

// 2026-11-04 本机实测：主通道只给 bytevc2，备用通道同一 vid 给 bytevc1。
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
        let play = fetch_model_light_batch(
            "single-private",
            1004,
            None,
            DecoderCapability::Full,
            &env,
            [&origin, &origin],
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(play.codec, "bytevc1");
        assert_eq!(play.definition, 1080);
        assert!(play.encrypted);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].path.contains(crate::params::SINGLE_MODEL_PATH));
        assert!(requests[1].path.contains(crate::params::MODEL_LIGHT_PATH));
        assert_eq!(
            requests[1].body["mixed_video_id_map"],
            json!({"1004": ["single-private"]})
        );
    }
}

#[tokio::test]
async fn single_playable_primary_does_not_call_fallback() {
    let (origin, server) = mock(vec![response(json!({"video_model": model("h264")}))]);
    let env = ApiEnv::anonymous(Default::default());
    let play = fetch_model_light_batch(
        "single-primary",
        1,
        None,
        DecoderCapability::NoHevc,
        &env,
        [&origin, &origin],
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(play.codec, "h264");
    assert_eq!(server.join().unwrap().len(), 1);
}

#[tokio::test]
async fn single_empty_or_missing_primary_still_falls_back() {
    for primary in [Vec::new(), response(json!({}))] {
        let (origin, server) = mock(vec![
            primary,
            response(json!({"single-empty": {"video_model": model("h264")}})),
        ]);
        let env = ApiEnv::anonymous(Default::default());
        let play = fetch_model_light_batch(
            "single-empty",
            1,
            None,
            DecoderCapability::NoHevc,
            &env,
            [&origin, &origin],
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(play.codec, "h264");
        assert_eq!(server.join().unwrap().len(), 2);
    }
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
        fetch_play_batch_at(&map, DecoderCapability::Full, &env, [&origin, &origin])
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
        serde_json::to_vec(&json!({"Code": 110001, "Message": "test failure"})).unwrap(),
    ]);
    let map = BTreeMap::from([("1".into(), vec!["batch-keep".into(), "batch-failed".into()])]);
    let env = ApiEnv::anonymous(Default::default());
    assert_eq!(
        fetch_play_batch_at(&map, DecoderCapability::Full, &env, [&origin, &origin])
            .await
            .unwrap(),
        1
    );
    assert_eq!(server.join().unwrap().len(), 2);
}

#[tokio::test]
async fn single_two_unplayable_responses_return_miss() {
    let (origin, server) = mock(vec![
        response(json!({"video_model": model("bytevc2")})),
        response(json!({"single-miss": {"video_model": model("bytevc2")}})),
    ]);
    let env = ApiEnv::anonymous(Default::default());
    assert!(
        fetch_model_light_batch(
            "single-miss",
            1004,
            None,
            DecoderCapability::Full,
            &env,
            [&origin, &origin]
        )
        .await
        .unwrap()
        .is_none()
    );
    assert_eq!(server.join().unwrap().len(), 2);
}

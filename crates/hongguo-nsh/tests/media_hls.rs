use super::MediaServer;
use hongguo_media::mux::{FragmentTiming, GrowingH264};
use std::path::PathBuf;

#[path = "media_readiness.rs"]
mod readiness_tests;

const GREY_IDR: &[u8] = &[
    0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x0a, 0xdd, 0xec, 0x04, 0x40, 0, 0, 3, 0, 0x40, 0, 0, 5, 0x23,
    0xc4, 0x89, 0xe0, 0, 0, 0, 1, 0x68, 0xce, 0x0f, 0xc8, 0, 0, 1, 0x65, 0x88, 0x84, 0x3a, 0x26,
    0x28, 0x0e,
];

struct Movie {
    path: PathBuf,
    writer: GrowingH264,
}

#[tokio::test]
async fn native_growing_eof_publishes_the_final_timeline_without_an_http_request() {
    use hongguo_nsh_video_api::read_source::{ReadResult, open_source};
    let movie = Movie::new();
    let server = MediaServer::start().await.unwrap();
    let lease = server
        .register_native(super::super::source::Source::file(&movie.path).unwrap())
        .unwrap();
    let reader = open_source(lease.url()).unwrap();
    assert_eq!(reader.size().unwrap(), None);
    assert_eq!(lease.available_duration().unwrap(), Some(0.0));
    hongguo_protocol::grow::finish(&movie.path);
    let length = reader.size().unwrap().unwrap();
    assert_eq!(reader.read_at(length, 1).unwrap(), ReadResult::Eof);
    assert_eq!(reader.read_at(length + 17, 1).unwrap(), ReadResult::Eof);
    assert!((lease.available_duration().unwrap().unwrap() - 6.0).abs() < 0.001);
    std::fs::remove_file(&movie.path).unwrap();
}

impl Movie {
    fn new() -> Self {
        let mut movie = Self::prefix();
        movie.append(30);
        movie
    }

    fn prefix() -> Self {
        let path = std::env::temp_dir().join(format!("hg-nsh-hls-{}.mp4", uuid::Uuid::new_v4()));
        let writer = GrowingH264::create(&path, 16, 16, None, 9.0).unwrap();
        hongguo_protocol::grow::open(&path).unwrap();
        let mut movie = Self { path, writer };
        movie.append(0);
        movie
    }

    fn append(&mut self, first: u32) {
        let frames: Vec<_> = (first..first + 30)
            .map(|index| (f64::from(index) / 10.0, GREY_IDR.to_vec(), true))
            .collect();
        let committed = self
            .writer
            .push(
                &frames,
                &[],
                FragmentTiming {
                    video_end_secs: f64::from(first + 30) / 10.0,
                    audio_end_secs: None,
                },
            )
            .unwrap();
        hongguo_protocol::grow::commit(&self.path, committed);
    }
}

#[tokio::test]
async fn hls_assets_have_real_lengths_ranges_and_the_same_lease_security_boundary() {
    let movie = Movie::new();
    let server = MediaServer::start().await.unwrap();
    let lease = server.file(&movie.path).unwrap();
    let base = lease.url().strip_suffix("playlist.m3u8").unwrap();
    let urls = [
        lease.url().to_owned(),
        format!("{base}init.mp4"),
        format!("{base}segment/0"),
    ];
    // Exercise the loopback server's Host/Origin boundary directly, independently of system proxies.
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for url in &urls {
        let denied = client
            .get(url)
            .header("Origin", "https://example.com")
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), 403);
        let wrong_host = client
            .get(url)
            .header("Host", "untrusted.invalid")
            .send()
            .await
            .unwrap();
        assert_eq!(wrong_host.status(), 400);
    }
    let init = client.get(&urls[1]).send().await.unwrap();
    let init_length = init.content_length().unwrap();
    let init = init.bytes().await.unwrap();
    assert_eq!(init_length, init.len() as u64);
    assert_eq!(&init[4..8], b"ftyp");
    let head = client.head(&urls[1]).send().await.unwrap();
    assert_eq!(head.headers()["content-length"], init_length.to_string());
    assert!(head.bytes().await.unwrap().is_empty());
    let segment = client
        .get(&urls[2])
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(&segment[4..8], b"moof");
    let range = client
        .get(&urls[2])
        .header("Range", "bytes=4-7")
        .send()
        .await
        .unwrap();
    assert_eq!(range.status(), 206);
    assert_eq!(
        range.headers()["content-range"],
        format!("bytes 4-7/{}", segment.len())
    );
    assert_eq!(range.content_length(), Some(4));
    assert_eq!(range.bytes().await.unwrap().as_ref(), b"moof");
    let suffix = client
        .get(&urls[2])
        .header("Range", "bytes=-3")
        .send()
        .await
        .unwrap();
    assert_eq!(
        suffix.bytes().await.unwrap().as_ref(),
        &segment[segment.len() - 3..]
    );
    let invalid = client
        .get(&urls[2])
        .header("Range", "bytes=99999999-")
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), 416);
    assert_eq!(
        invalid.headers()["content-range"],
        format!("bytes */{}", segment.len())
    );
    assert_eq!(
        client
            .get(format!("{base}segment/1"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    drop(lease);
    for url in &urls {
        assert_eq!(client.get(url).send().await.unwrap().status(), 404);
    }
}

#[tokio::test]
async fn incomplete_idr_groups_wait_for_a_committed_boundary_and_stop_when_revoked() {
    let mut movie = Movie::prefix();
    let server = MediaServer::start().await.unwrap();
    let lease = server.file(&movie.path).unwrap();
    let url = lease.url().to_owned();
    let mut request = tokio::spawn(async move { reqwest::get(url).await.unwrap() });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut request)
            .await
            .is_err()
    );
    movie.append(30);
    assert_eq!(request.await.unwrap().status(), 200);
    let second = Movie::prefix();
    let lease = server.file(&second.path).unwrap();
    let url = lease.url().to_owned();
    let mut waiting = tokio::spawn(async move { reqwest::get(url).await.unwrap() });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut waiting)
            .await
            .is_err()
    );
    drop(lease);
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap()
            .status(),
        409
    );
}

#[tokio::test]
async fn failed_or_cancelled_hls_never_publishes_endlist_or_old_segments() {
    for cancelled in [false, true] {
        let movie = Movie::new();
        let server = MediaServer::start().await.unwrap();
        let lease = server.file(&movie.path).unwrap();
        let base = lease.url().strip_suffix("playlist.m3u8").unwrap();
        if cancelled {
            hongguo_protocol::grow::cancel(&movie.path);
        } else {
            hongguo_protocol::grow::fail(&movie.path, "encoder failure");
        }
        assert!(server.file(&movie.path).is_err());
        assert!(lease.is_complete().is_err());
        for url in [
            lease.url().to_owned(),
            format!("{base}init.mp4"),
            format!("{base}segment/0"),
        ] {
            let response = reqwest::get(url).await.unwrap();
            assert_eq!(response.status(), if cancelled { 409 } else { 500 });
            assert!(!response.text().await.unwrap().contains("#EXT-X-ENDLIST"));
        }
    }
}

#[tokio::test]
async fn a_gop_exceeding_the_fixed_target_is_rejected_without_mutating_the_playlist_contract() {
    let path = std::env::temp_dir().join(format!("hg-nsh-long-gop-{}.mp4", uuid::Uuid::new_v4()));
    let mut writer = GrowingH264::create(&path, 16, 16, None, 7.0).unwrap();
    hongguo_protocol::grow::open(&path).unwrap();
    let frames: Vec<_> = (0..70)
        .map(|at| (f64::from(at) / 10.0, GREY_IDR.to_vec(), at == 0))
        .collect();
    let len = writer
        .push(
            &frames,
            &[],
            FragmentTiming {
                video_end_secs: 7.0,
                audio_end_secs: None,
            },
        )
        .unwrap();
    hongguo_protocol::grow::commit(&path, len);
    let server = MediaServer::start().await.unwrap();
    let lease = server.file(&path).unwrap();
    hongguo_protocol::grow::finish(&path);
    let response = reqwest::get(lease.url()).await.unwrap();
    assert_eq!(response.status(), 500);
    assert!(!response.text().await.unwrap().contains("#EXT-X-ENDLIST"));
    std::fs::remove_file(path).unwrap();
}

impl Drop for Movie {
    fn drop(&mut self) {
        hongguo_protocol::grow::cancel(&self.path);
        let _ = std::fs::remove_file(&self.path);
    }
}

#[tokio::test]
async fn growing_fmp4_uses_an_event_playlist_before_completion() {
    let mut movie = Movie::new();
    let server = MediaServer::start().await.unwrap();
    let lease = server.file(&movie.path).unwrap();
    assert!(lease.url().ends_with("/playlist.m3u8"));
    assert_eq!(lease.available_duration().unwrap(), Some(0.0));
    let client = reqwest::Client::new();
    let response = client.get(lease.url()).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()["content-type"],
        "application/vnd.apple.mpegurl"
    );
    let playlist = response.text().await.unwrap();
    assert!(playlist.contains("#EXT-X-PLAYLIST-TYPE:EVENT"));
    assert!(playlist.contains("#EXT-X-MAP:URI=\"init.mp4\""));
    assert!(playlist.contains("#EXTINF:3.000000,\nsegment/0\n"));
    assert!(!playlist.contains("#EXT-X-ENDLIST"));
    assert!(!lease.is_complete().unwrap());
    assert_eq!(lease.available_duration().unwrap(), Some(3.0));
    movie.append(60);
    hongguo_protocol::grow::finish(&movie.path);
    let complete = client
        .get(lease.url())
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(complete.contains("segment/1\n"));
    assert!(complete.ends_with("#EXT-X-ENDLIST\n"));
    assert!(lease.is_complete().unwrap());
    assert_eq!(lease.available_duration().unwrap(), Some(9.0));
    assert_eq!(
        server
            .file(&movie.path)
            .unwrap()
            .available_duration()
            .unwrap(),
        None
    );
}

use super::MediaServer;

#[tokio::test]
async fn native_audio_http_view_preserves_original_samples_and_shares_revocation() {
    let path = std::env::temp_dir().join(format!("hg-audio-view-{}.mp4", uuid::Uuid::new_v4()));
    let original = include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");
    std::fs::write(&path, original).unwrap();
    let server = MediaServer::start().await.unwrap();
    let lease = server.playback_file(&path).unwrap();
    let url = format!("{}/audio", lease.url());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client.get(&url).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let masked = response.bytes().await.unwrap();
    assert_eq!(masked.len(), original.len());
    let media = hongguo_media::demux::demux_bytes(&masked).unwrap();
    assert!(media.video_track().is_none());
    let audio = media.audio_track().unwrap();
    let source = hongguo_media::demux::demux_bytes(original).unwrap();
    assert_eq!(
        audio.info.samples,
        source.audio_track().unwrap().info.samples
    );
    for &(offset, size) in &audio.info.samples {
        let range = offset as usize..(offset + size) as usize;
        assert_eq!(masked[range.clone()], original[range]);
    }
    let head = client.head(&url).send().await.unwrap();
    assert_eq!(head.headers()["content-length"], original.len().to_string());
    let partial = client
        .get(&url)
        .header("Range", "bytes=1-37")
        .send()
        .await
        .unwrap();
    assert_eq!(partial.status(), 206);
    assert_eq!(partial.bytes().await.unwrap().as_ref(), &masked[1..38]);
    assert_eq!(
        client
            .get(&url)
            .header("Origin", "http://example.com")
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    drop(lease);
    assert_eq!(client.get(url).send().await.unwrap().status(), 404);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn h264_is_readable_by_native_video_without_a_frame_session() {
    let path = std::env::temp_dir().join(format!("hg-nsh-rejected-{}.mp4", uuid::Uuid::new_v4()));
    std::fs::write(
        &path,
        include_bytes!("../../hongguo-softdec/tests/fixtures/h264-64x48-aac.mp4"),
    )
    .unwrap();
    let server = MediaServer::start().await.unwrap();
    let lease = server.playback_file(&path).unwrap();
    assert!(lease.frame_source().is_none());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client.get(lease.url()).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.bytes().await.unwrap().as_ref(),
        include_bytes!("../../hongguo-softdec/tests/fixtures/h264-64x48-aac.mp4")
    );
    drop(lease);
    assert!(server.routes.sources.lock().unwrap().is_empty());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn codec_extension_decodes_the_same_http_source_and_retires_its_frame_lease() {
    let path = std::env::temp_dir().join(format!("hg-nsh-direct-{}.mp4", uuid::Uuid::new_v4()));
    let original = include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");
    std::fs::write(&path, original).unwrap();
    let server = MediaServer::start().await.unwrap();
    let lease = server.playback_file(&path).unwrap();
    let handle = lease.enable_codec_extension(0.0).unwrap();
    let frames = hongguo_nsh_video_api::frames::open_frames(&handle).unwrap();
    frames.configure_decoder(None).unwrap();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(frame) = frames.native_frame_at(0.0).unwrap() {
                break frame;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        (
            frame.image.descriptor().width,
            frame.image.descriptor().height
        ),
        (136, 76)
    );
    assert!(frames.holding().unwrap());
    assert!(frames.presented(frame.generation, frame.serial).unwrap());
    assert!(lease.url().starts_with("http://127.0.0.1:"));
    let source = server.routes.sources.lock().unwrap()[&lease.id].clone();
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    assert_eq!(
        source.read(0, original.len() as u64, &cancelled).unwrap(),
        original
    );
    drop(lease);
    assert!(frames.frame_at(0).is_err());
    assert!(frames.native_frame_at(0.0).is_err());
    assert!(source.read(0, 1, &cancelled).is_err());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn native_hevc_keeps_the_original_tracks_and_does_not_start_a_frame_decoder() {
    let path = std::env::temp_dir().join(format!("hg-nsh-native-{}.mp4", uuid::Uuid::new_v4()));
    let original = include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");
    std::fs::write(&path, original).unwrap();
    let server = MediaServer::start().await.unwrap();
    let lease = server.playback_file(&path).unwrap();
    assert!(lease.frame_source().is_none());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    assert_eq!(
        client
            .get(lease.url())
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
            .as_ref(),
        original
    );
    let url = lease.url().to_owned();
    drop(lease);
    assert_eq!(client.get(url).send().await.unwrap().status(), 404);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn revoking_media_stops_existing_readers_without_affecting_other_leases() {
    let path = std::env::temp_dir().join(format!("hg-nsh-{}.mp4", uuid::Uuid::new_v4()));
    std::fs::write(&path, b"0123456789").unwrap();
    let server = MediaServer::start().await.unwrap();
    let lease = server.file(&path).unwrap();
    let other = server.file(&path).unwrap();
    let source = server.routes.sources.lock().unwrap()[&lease.id].clone();
    let other_source = server.routes.sources.lock().unwrap()[&other.id].clone();
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    assert_eq!(source.read(0, 1, &cancelled).unwrap(), b"0");
    drop(lease);
    assert!(source.read(0, 1, &cancelled).is_err());
    assert_eq!(other_source.read(0, 1, &cancelled).unwrap(), b"0");
    drop(server);
    assert!(other_source.read(0, 1, &cancelled).is_err());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn media_urls_are_scoped_and_support_head_and_ranges() {
    let path = std::env::temp_dir().join(format!("hg-nsh-{}.mp4", uuid::Uuid::new_v4()));
    std::fs::write(&path, b"0123456789").unwrap();
    let server = MediaServer::start().await.unwrap();
    let lease = server.file(&path).unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client
        .get(lease.url())
        .header("Range", "bytes=2-5")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 206);
    assert_eq!(response.headers()["content-range"], "bytes 2-5/10");
    assert_eq!(response.bytes().await.unwrap().as_ref(), b"2345");
    let head = client.head(lease.url()).send().await.unwrap();
    assert_eq!(head.status(), 200);
    assert_eq!(head.headers()["content-length"], "10");
    assert!(head.bytes().await.unwrap().is_empty());
    let invalid = client
        .get(lease.url())
        .header("Range", "bytes=30-")
        .send()
        .await
        .unwrap();
    assert_eq!(invalid.status(), 416);
    assert_eq!(invalid.headers()["content-range"], "bytes */10");
    let full = client.get(lease.url()).send().await.unwrap();
    assert_eq!(full.bytes().await.unwrap().as_ref(), b"0123456789");
    let denied = client
        .get(lease.url())
        .header("Origin", "https://example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);
    let url = lease.url().to_owned();
    drop(lease);
    assert_eq!(client.get(url).send().await.unwrap().status(), 404);
    std::fs::remove_file(path).unwrap();
}

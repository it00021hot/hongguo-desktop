use super::MediaServer;

#[tokio::test]
async fn direct_media_serves_only_audio_while_registered_frames_decode_original_video() {
    let path = std::env::temp_dir().join(format!("hg-nsh-direct-{}.mp4", uuid::Uuid::new_v4()));
    let original = include_bytes!("../../hongguo-softdec/tests/fixtures/hevc-main-136x76-aac.mp4");
    std::fs::write(&path, original).unwrap();
    let server = MediaServer::start().await.unwrap();
    let lease = server.playback_file(&path, true).unwrap();
    let frames = hongguo_nsh_video_api::frames::open_frames(lease.frame_source().unwrap()).unwrap();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Some(frame) = frames.frame_at(0).unwrap() {
                break frame;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!((frame.width, frame.height), (136, 76));
    assert!(
        frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
    assert!(frames.holding().unwrap());
    assert!(frames.presented(frame.generation, frame.serial).unwrap());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let audio = client
        .get(lease.url())
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(audio.len(), original.len());
    let index =
        hongguo_softdec::IndexedVideo::open(original.len() as u64, |offset, length, out| {
            out.clear();
            out.extend_from_slice(&original[offset as usize..(offset + length) as usize]);
            Ok(())
        })
        .unwrap();
    let mut masked = original.to_vec();
    index.mask_video_tracks(0, &mut masked);
    assert_ne!(masked, original);
    assert_eq!(audio.as_ref(), masked);
    let url = lease.url().to_owned();
    drop(lease);
    assert!(frames.frame_at(0).is_err());
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

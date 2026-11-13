use super::MediaServer;

#[path = "media_hls.rs"]
mod hls_tests;
#[path = "media_native_read.rs"]
mod native_read_tests;

#[tokio::test]
async fn a_media_lease_reports_only_a_successfully_finalized_source() {
    let path = std::env::temp_dir().join(format!("hg-nsh-finalized-{}.mp4", uuid::Uuid::new_v4()));
    std::fs::write(&path, b"prefix").unwrap();
    hongguo_protocol::grow::open(&path).unwrap();
    hongguo_protocol::grow::commit(&path, 6);
    let server = MediaServer::start().await.unwrap();
    let lease = server.file(&path).unwrap();
    assert!(!lease.is_complete().unwrap());
    hongguo_protocol::grow::finish(&path);
    assert!(lease.is_complete().unwrap());
    let fixed = server.file(&path).unwrap();
    assert!(fixed.is_complete().unwrap());
    drop(server);
    assert!(lease.is_complete().is_err());
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn an_empty_finished_growing_source_has_a_range_error_instead_of_panicking() {
    let path = std::env::temp_dir().join(format!("hg-nsh-empty-{}.mp4", uuid::Uuid::new_v4()));
    std::fs::write(&path, b"").unwrap();
    hongguo_protocol::grow::open(&path).unwrap();
    let server = MediaServer::start().await.unwrap();
    let lease = server.file(&path).unwrap();
    hongguo_protocol::grow::finish(&path);
    let response = reqwest::get(lease.url()).await.unwrap();
    assert_eq!(response.status(), 416);
    assert_eq!(response.headers()["content-range"], "bytes */0");
    assert!(response.bytes().await.unwrap().is_empty());
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
    let client = reqwest::Client::new();
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

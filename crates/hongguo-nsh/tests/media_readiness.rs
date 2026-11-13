use super::{MediaServer, Movie};
use std::time::Duration;

#[tokio::test]
async fn a_paused_source_waits_for_real_published_media_without_native_ticks_or_http_reads() {
    let mut movie = Movie::prefix();
    let server = MediaServer::start().await.unwrap();
    let lease = server.file(&movie.path).unwrap();
    let readiness = lease.readiness().unwrap().unwrap();
    let mut waiting = tokio::spawn(async move { readiness.wait_for_available(4.0).await });
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut waiting)
            .await
            .is_err()
    );
    movie.append(30);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut waiting)
            .await
            .is_err()
    );
    movie.append(60);
    tokio::time::timeout(Duration::from_secs(2), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(lease.available_duration().unwrap(), Some(6.0));
    assert!(!lease.is_complete().unwrap());
}

#[tokio::test]
async fn readiness_ends_explicitly_when_revoked_failed_cancelled_or_finally_out_of_range() {
    for ending in ["revoked", "failed", "cancelled", "done"] {
        let movie = Movie::prefix();
        let server = MediaServer::start().await.unwrap();
        let lease = server.file(&movie.path).unwrap();
        let readiness = lease.readiness().unwrap().unwrap();
        let mut waiting = tokio::spawn(async move { readiness.wait_for_available(3.0).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(30), &mut waiting)
                .await
                .is_err()
        );
        match ending {
            "revoked" => drop(lease),
            "failed" => hongguo_protocol::grow::fail(&movie.path, "test producer failure"),
            "cancelled" => hongguo_protocol::grow::cancel(&movie.path),
            "done" => hongguo_protocol::grow::finish(&movie.path),
            _ => unreachable!(),
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(2), waiting)
                .await
                .unwrap()
                .unwrap()
                .is_err(),
            "{ending} must terminate the wait"
        );
    }
}

#[tokio::test]
async fn dropping_one_wait_does_not_revoke_the_lease_or_other_readiness_clones() {
    let mut movie = Movie::prefix();
    let server = MediaServer::start().await.unwrap();
    let lease = server.file(&movie.path).unwrap();
    let first = lease.readiness().unwrap().unwrap();
    let second = first.clone();
    let mut abandoned = tokio::spawn(async move { first.wait_for_available(8.0).await });
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut abandoned)
            .await
            .is_err()
    );
    abandoned.abort();
    assert!(abandoned.await.unwrap_err().is_cancelled());
    assert!(!lease.is_complete().unwrap());
    movie.append(30);
    tokio::time::timeout(Duration::from_secs(2), second.wait_for_available(1.0))
        .await
        .unwrap()
        .unwrap();
    drop(lease);
    assert!(second.wait_for_available(1.0).await.is_err());
}

#[tokio::test]
async fn readiness_rejects_invalid_targets_and_fixed_files_need_no_waiter() {
    let movie = Movie::new();
    let server = MediaServer::start().await.unwrap();
    let lease = server.file(&movie.path).unwrap();
    let readiness = lease.readiness().unwrap().unwrap();
    for target in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        assert!(readiness.wait_for_available(target).await.is_err());
    }
    hongguo_protocol::grow::finish(&movie.path);
    assert!(
        server
            .file(&movie.path)
            .unwrap()
            .readiness()
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn dropping_the_cancellation_guard_releases_a_condvar_worker_without_another_commit() {
    use crate::media::{cancellation::Cancellation, source::Source};
    let movie = Movie::prefix();
    let source = Source::file(&movie.path).unwrap();
    let cancellation = Cancellation::default();
    let flag = cancellation.flag();
    let mut worker =
        hongguo_service::runtime::spawn_blocking(move || source.wait_for_available(4.0, &flag));
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut worker)
            .await
            .is_err()
    );
    drop(cancellation);
    assert!(
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
}

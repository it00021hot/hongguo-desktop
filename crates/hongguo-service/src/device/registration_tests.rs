use std::sync::Arc;
use std::time::Duration;

use hongguo_common::utils::time::now_ms;
use hongguo_store::Store;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use super::*;
use crate::app_state::AppStateInner;

#[tokio::test]
async fn first_network_failure_returns_before_the_retry_pause() {
    let db = Store::open_memory().unwrap();
    write_meta_strict(&db, &BootstrapMeta::default()).unwrap();
    let state = Arc::new(AppStateInner::with_db(db));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());

    let request = register_and_swap_at(&state, "测试首次失败", &origin, Duration::from_secs(60));
    let (_, result) = tokio::join!(
        respond(&listener, "503 Service Unavailable", ""),
        tokio::time::timeout(Duration::from_secs(1), request),
    );

    let first = result.expect("第一次网络失败应立即返回，不能等待后续一分钟退避");
    assert!(!first.ok);
    assert!(first.network);
    assert!(!hongguo_api::register::has_registered_ids(&state.device()));
    for _ in 0..3 {
        ensure_registered(&state).await;
        assert!(matches!(
            crate::cold_start::wait(&state).await,
            Err(hongguo_common::AppError::Network(_))
        ));
    }
    let status = registration_status(&state);
    assert_eq!(status.phase, RegistrationPhase::RetryScheduled);
    assert_eq!(status.attempt, 1);
    assert!(status.next_retry_at.is_some());
}

#[tokio::test]
async fn concurrent_manual_retries_share_one_task_and_identity() {
    let state = trusted_state();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mut changes = subscribe_registration(&state);
    let joined = tokio::sync::Notify::new();

    let server = async {
        let first = respond(&listener, "503 Service Unavailable", "").await;
        joined.notified().await;
        let second = respond(&listener, "200 OK", &successful_body()).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        (first, second)
    };
    let requests = async {
        let first = register_and_swap_at(&state, "首次", &origin, Duration::from_secs(60)).await;
        assert!(first.network);
        let (left, right) = tokio::join!(
            register_and_swap_at(&state, "手动左", &origin, Duration::from_secs(60)),
            register_and_swap_at(&state, "手动右", &origin, Duration::from_secs(60)),
        );
        assert!(left.network && right.network);
        joined.notify_one();
        wait_phase(&mut changes, RegistrationPhase::Ready).await;
    };
    let ((first, second), ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(server, requests)
    })
    .await
    .unwrap();

    assert_eq!(first.target, second.target);
    for field in ["cdid", "openudid", "clientudid", "req_id"] {
        assert_eq!(first.body["header"][field], second.body["header"][field]);
    }
    assert_eq!(
        state.device().get("cdid"),
        first.body["header"]["cdid"].as_str().unwrap()
    );
    assert_eq!(registration_status(&state).attempt, 2);
    crate::cold_start::wait(&state).await.unwrap();
}

#[tokio::test]
async fn valid_existing_identity_remains_playable_under_manual_registration_block() {
    let db = Store::open_memory().unwrap();
    let profile = registered_profile(&RegisterResult {
        device_id: "8888888888888888".into(),
        install_id: "9999999999999999".into(),
        cdid: hongguo_signer::fresh_cdid(),
        openudid: "0011223344556677".into(),
        ttreq: "test-ticket".into(),
        ..RegisterResult::default()
    });
    write_meta_strict(
        &db,
        &BootstrapMeta {
            register_block_manual: true,
            ..BootstrapMeta::default()
        },
    )
    .unwrap();
    let state = Arc::new(AppStateInner::with_device(db, profile.clone()));

    ensure_registered(&state).await;
    crate::cold_start::wait(&state).await.unwrap();

    assert_eq!(state.device(), profile);
    assert_eq!(
        registration_status(&state).phase,
        RegistrationPhase::Blocked
    );
    assert!(registration_status(&state).is_manual_block);
    assert!(read_meta(&state.store).register_block_manual);
}

#[tokio::test]
async fn server_registration_limits_survive_reopen_and_manual_retry() {
    let root = std::env::temp_dir().join(format!("hg-registration-limits-{}", std::process::id()));
    for (name, headers) in [("manual", ""), ("timed", "Retry-After: 60\r\n")] {
        let directory = root.join(name);
        std::fs::create_dir_all(&directory).unwrap();
        let db = Store::open(directory.join("device.db")).unwrap();
        write_meta_strict(&db, &BootstrapMeta::default()).unwrap();
        let state = Arc::new(AppStateInner::with_db(db));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (_, result) = tokio::join!(
            respond_with_headers(&listener, "429 Too Many Requests", "", headers),
            register_and_swap_at(&state, "服务端限制", &origin, Duration::ZERO),
        );
        assert!(!result.ok);
        let before = read_meta(&state.store);
        assert!(registration_blocked(&before, now_ms()));
        drop(state);
        let db = Store::open(directory.join("device.db")).unwrap();
        let state = Arc::new(AppStateInner::with_db(db));

        ensure_registered(&state).await;
        let retried = register_and_swap_at(&state, "重开后重试", &origin, Duration::ZERO).await;

        assert!(!retried.ok);
        assert!(matches!(
            crate::cold_start::wait(&state).await,
            Err(hongguo_common::AppError::Rejected(_))
        ));
        let after = read_meta(&state.store);
        assert_eq!(before.register_block_manual, after.register_block_manual);
        assert_eq!(
            before.register_block_until_ms,
            after.register_block_until_ms
        );
        assert_eq!(before.register_block_seq, after.register_block_seq);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        drop(state);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn exhausted_startup_is_not_restarted_by_playback_waits() {
    let state = trusted_state();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mut changes = subscribe_registration(&state);
    let server = async {
        for _ in 0..REGISTER_RETRY_MAX {
            respond(&listener, "503 Service Unavailable", "").await;
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    };
    let requests = async {
        let first = register_and_swap_at(&state, "耗尽重试", &origin, Duration::ZERO).await;
        assert!(first.network);
        wait_phase(&mut changes, RegistrationPhase::Failed).await;
        for _ in 0..3 {
            ensure_registered(&state).await;
            assert!(crate::cold_start::wait(&state).await.is_err());
        }
        assert_eq!(registration_status(&state).attempt, REGISTER_RETRY_MAX);
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(server, requests)
    })
    .await
    .unwrap();
}

fn trusted_state() -> AppState {
    let db = Store::open_memory().unwrap();
    write_meta_strict(&db, &BootstrapMeta::default()).unwrap();
    Arc::new(AppStateInner::with_db(db))
}

fn successful_body() -> String {
    serde_json::json!({"device_id": "8888888888888888", "install_id": "9999999999999999"})
        .to_string()
}

async fn wait_phase(
    changes: &mut tokio::sync::watch::Receiver<RegistrationStatus>,
    expected: RegistrationPhase,
) {
    loop {
        if changes.borrow_and_update().phase == expected {
            return;
        }
        changes.changed().await.unwrap();
    }
}

struct ObservedRequest {
    target: String,
    body: Value,
}

async fn respond(listener: &TcpListener, status: &str, body: &str) -> ObservedRequest {
    respond_with_headers(listener, status, body, "").await
}

async fn respond_with_headers(
    listener: &TcpListener,
    status: &str,
    body: &str,
    extra_headers: &str,
) -> ObservedRequest {
    let (socket, _) = listener.accept().await.unwrap();
    let mut socket = BufReader::new(socket);
    let mut headers = String::new();
    while !headers.ends_with("\r\n\r\n") {
        assert_ne!(socket.read_line(&mut headers).await.unwrap(), 0);
    }
    let length = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .unwrap()
        .parse()
        .unwrap();
    let mut encrypted = vec![0; length];
    socket.read_exact(&mut encrypted).await.unwrap();
    let (plaintext, _) = hongguo_api::register::tt_decrypt_v5(&encrypted).unwrap();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
        body.len(),
    );
    socket.write_all(response.as_bytes()).await.unwrap();
    ObservedRequest {
        target: headers.lines().next().unwrap().into(),
        body: serde_json::from_slice(&plaintext).unwrap(),
    }
}

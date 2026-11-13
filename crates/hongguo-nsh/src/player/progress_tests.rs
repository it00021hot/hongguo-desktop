use super::*;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn closing_a_source_saves_its_latest_position_before_the_interval() {
    let (sender, receiver) = tokio::sync::watch::channel(Update::default());
    let writes = Arc::new(Mutex::new(Vec::new()));
    let saved = writes.clone();
    let worker = tokio::spawn(run_writer(receiver, move |position| {
        saved.lock().unwrap().push(position);
        std::future::ready(Ok(()))
    }));
    sender.send_replace(Update {
        position: ProgressPosition::new(12.5, 60.0),
        flush: 0,
    });
    sender.send_replace(Update {
        position: ProgressPosition::new(18.75, 60.0),
        flush: 0,
    });
    drop(sender);
    worker.await.unwrap();
    assert_eq!(
        *writes.lock().unwrap(),
        vec![ProgressPosition::new(18.75, 60.0).unwrap()]
    );
}

#[tokio::test]
async fn a_hidden_source_flushes_and_unchanged_positions_are_not_written_again() {
    let (sender, receiver) = tokio::sync::watch::channel(Update::default());
    let (saved, mut writes) = tokio::sync::mpsc::unbounded_channel();
    let worker = tokio::spawn(run_writer(receiver, move |position| {
        saved.send(position).unwrap();
        std::future::ready(Ok(()))
    }));
    let position = ProgressPosition::new(9.0, 60.0);
    sender.send_replace(Update { position, flush: 1 });
    assert_eq!(writes.recv().await, position);
    sender.send_replace(Update { position, flush: 2 });
    drop(sender);
    worker.await.unwrap();
    assert!(writes.recv().await.is_none());
}

#[tokio::test]
async fn an_unplayed_source_has_no_position_to_save() {
    let (sender, receiver) = tokio::sync::watch::channel(Update::default());
    drop(sender);
    run_writer(receiver, |_| async {
        panic!("an unloaded source must not write history")
    })
    .await;
}

#[tokio::test]
async fn a_failed_save_does_not_mark_the_position_as_persisted() {
    let (sender, receiver) = tokio::sync::watch::channel(Update::default());
    let (completed, mut attempts) = tokio::sync::mpsc::unbounded_channel();
    let mut calls = 0;
    let worker = tokio::spawn(run_writer(receiver, move |position| {
        calls += 1;
        completed.send(position).unwrap();
        std::future::ready(if calls == 1 {
            Err(AppError::Io("injected write failure".into()))
        } else {
            Ok(())
        })
    }));
    let position = ProgressPosition::new(4.0, 60.0);
    sender.send_replace(Update { position, flush: 1 });
    assert_eq!(attempts.recv().await, position);
    drop(sender);
    worker.await.unwrap();
    assert_eq!(attempts.recv().await, position);
    assert!(attempts.recv().await.is_none());
}

#[test]
fn invalid_native_positions_are_rejected_and_end_positions_are_clamped() {
    for (at, duration) in [
        (f64::NAN, 60.0),
        (1.0, f64::INFINITY),
        (-1.0, 60.0),
        (1.0, 0.0),
    ] {
        assert!(ProgressPosition::new(at, duration).is_none());
    }
    assert_eq!(ProgressPosition::new(0.0, 60.0).unwrap().at, 0.0);
    assert_eq!(ProgressPosition::new(60.5, 60.0).unwrap().at, 60.0);
}

#[tokio::test]
async fn a_slow_cloud_report_does_not_block_local_saves_and_still_reports_the_final_sample() {
    let (sender, receiver) = watch::channel(Update::default());
    let (local_saved, mut local_writes) = tokio::sync::mpsc::unbounded_channel();
    let local = tokio::spawn(run_writer(receiver.clone(), move |position| {
        local_saved.send(position).unwrap();
        std::future::ready(Ok(()))
    }));
    let release = Arc::new(tokio::sync::Notify::new());
    let cloud_gate = release.clone();
    let (cloud_started, mut cloud_writes) = tokio::sync::mpsc::unbounded_channel();
    let cloud = tokio::spawn(run_interval_writer(
        receiver,
        CLOUD_INTERVAL,
        move |position| {
            cloud_started.send(position).unwrap();
            let gate = cloud_gate.clone();
            async move {
                if position.at == 1.0 {
                    gate.notified().await;
                }
                Ok(())
            }
        },
    ));
    let first = ProgressPosition::new(1.0, 60.0);
    let final_position = ProgressPosition::new(20.0, 60.0);
    sender.send_replace(Update {
        position: first,
        flush: 1,
    });
    assert_eq!(cloud_writes.recv().await, first);
    assert_eq!(local_writes.recv().await, first);
    sender.send_replace(Update {
        position: final_position,
        flush: 2,
    });
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), local_writes.recv())
            .await
            .unwrap(),
        final_position
    );
    drop(sender);
    release.notify_one();
    local.await.unwrap();
    cloud.await.unwrap();
    assert_eq!(cloud_writes.recv().await, final_position);
    assert!(cloud_writes.recv().await.is_none());
}

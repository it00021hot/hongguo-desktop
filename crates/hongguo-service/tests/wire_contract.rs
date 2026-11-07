//! 跨端事件载荷的 wire 形状快照（架构二期重构的回归锁）。
//!
//! 这些结构序列化后直接进 Tauri 事件（前端 zod schema 镜像校验），形状
//! 漂移是静默事故——平台拆分、service 去 Tauri 化、事件通道迁移期间必须
//! 逐字段不变。断言用 `serde_json::Value` 全量比对：缺字段、多字段、改名、
//! 改类型都会红；与前端 schema 的对应关系见 `service/tauri/types.ts`。

use hongguo_service::download_service::events::{
    ProgressEvent, QueueChangedEvent, TaskEvent, TaskFailureEvent,
};
use hongguo_service::follow_service::events::FollowNoticesEvent;
use hongguo_service::login_service::events::LoginMfaStateEvent;
use hongguo_service::play_service::compat_play::{
    CompatPhase, CompatPlayResult, CompatProgressEvent,
};
use hongguo_service::play_service::online::OnlineProgressEvent;
use serde_json::json;

#[test]
fn download_progress_event_shape() {
    let v = serde_json::to_value(ProgressEvent {
        id: "t1".into(),
        downloaded: 10,
        total: 100,
        percent: 10.0,
    })
    .unwrap();
    assert_eq!(
        v,
        json!({ "id": "t1", "downloaded": 10, "total": 100, "percent": 10.0 })
    );
}

#[test]
fn download_task_events_shape() {
    let v = serde_json::to_value(TaskEvent { id: "t1".into() }).unwrap();
    assert_eq!(v, json!({ "id": "t1" }));

    let v = serde_json::to_value(TaskFailureEvent {
        id: "t1".into(),
        error: "error.network".into(),
    })
    .unwrap();
    assert_eq!(v, json!({ "id": "t1", "error": "error.network" }));
}

#[test]
fn download_queue_changed_event_is_empty_object() {
    let v = serde_json::to_value(QueueChangedEvent {}).unwrap();
    assert_eq!(v, json!({}));
}

#[test]
fn online_progress_event_shape() {
    let v = serde_json::to_value(OnlineProgressEvent {
        key: "7687919221593885758:3".into(),
        received: 2048,
        total: 4096,
        percent: 50.0,
        phase: "downloading".into(),
    })
    .unwrap();
    assert_eq!(
        v,
        json!({
            "key": "7687919221593885758:3",
            "received": 2048,
            "total": 4096,
            "percent": 50.0,
            "phase": "downloading",
        })
    );
}

#[test]
fn login_mfa_state_event_shape() {
    let v = serde_json::to_value(LoginMfaStateEvent::Success { name: "甲".into() }).unwrap();
    assert_eq!(v, json!({ "state": "success", "name": "甲" }));

    let v = serde_json::to_value(LoginMfaStateEvent::Failed {
        message_kind: "settings.loginMfaTimeout".into(),
        message: String::new(),
    })
    .unwrap();
    assert_eq!(
        v,
        json!({ "state": "failed", "messageKind": "settings.loginMfaTimeout", "message": "" })
    );

    let v = serde_json::to_value(LoginMfaStateEvent::Waiting {
        server_tip: "回复 YZ 到 10691".into(),
    })
    .unwrap();
    assert_eq!(
        v,
        json!({ "state": "waiting", "serverTip": "回复 YZ 到 10691" })
    );
}

#[test]
fn follow_notices_event_shape() {
    // 追更更新提醒事件（前端 `followNoticesEventSchema` 镜像）：通知数组
    // camelCase 全字段（id/kind/seriesId/title/detail/at/read）。
    let v = serde_json::to_value(FollowNoticesEvent {
        notices: vec![hongguo_common::model::FollowNotice {
            id: 7,
            kind: "follow".into(),
            series_id: "7677818497497320472".into(),
            title: "摆摊大王".into(),
            detail: "追的剧更新了，新增 2 集（共 216 集）".into(),
            at: 1_792_041_548,
            read: false,
        }],
    })
    .unwrap();
    assert_eq!(
        v,
        json!({
            "notices": [{
                "id": 7,
                "kind": "follow",
                "seriesId": "7677818497497320472",
                "title": "摆摊大王",
                "detail": "追的剧更新了，新增 2 集（共 216 集）",
                "at": 1_792_041_548i64,
                "read": false
            }]
        })
    );
}

/// 兼容转码进度事件的 wire 形状（前端 `compatProgressSchema` 镜像）。
/// 架构二期把本类型自 tauri 命令移入 service，断言逐步沿用。
#[test]
fn compat_progress_event_shape() {
    let v = serde_json::to_value(CompatProgressEvent {
        key: "1:2".into(),
        percent: 42.5,
        phase: CompatPhase::Transcoding,
    })
    .unwrap();
    assert_eq!(
        v,
        json!({ "key": "1:2", "percent": 42.5, "phase": "transcoding" })
    );
}

#[test]
fn compat_phase_serializes_lowercase() {
    assert_eq!(
        serde_json::to_string(&CompatPhase::Downloading).unwrap(),
        "\"downloading\""
    );
    assert_eq!(
        serde_json::to_string(&CompatPhase::Ready).unwrap(),
        "\"ready\""
    );
    assert_eq!(
        serde_json::to_string(&CompatPhase::Completed).unwrap(),
        "\"completed\""
    );
    assert_eq!(
        serde_json::to_string(&CompatPhase::Failed).unwrap(),
        "\"failed\""
    );
}

/// 兜底转码的返回形状（前端 `compatPlayResultSchema` 镜像）。
#[test]
fn compat_play_result_shape() {
    let v = serde_json::to_value(CompatPlayResult {
        url: "hongguo-local://stream/1".into(),
        cached: true,
        backend: "VT".into(),
        elapsed_ms: 0,
        progressive: false,
    })
    .unwrap();
    assert_eq!(
        v,
        json!({
            "url": "hongguo-local://stream/1",
            "cached": true,
            "backend": "VT",
            "elapsedMs": 0,
            "progressive": false,
        })
    );
}

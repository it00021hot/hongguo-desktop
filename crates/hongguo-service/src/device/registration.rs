use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use hongguo_api::client::ApiEnv;
use hongguo_api::register::{RegisterAttemptError, RegisterMaterial};
use hongguo_common::AppError;
use hongguo_common::utils::time::now_ms;
use hongguo_signer::device::anonymous_profile;

use super::{
    REGISTER_FORCE_BLOCK, REGISTER_RETRY_MAX, REGISTER_RETRY_MS, Registration, RegistrationPhase,
    RegistrationStatus, activate_registration, persist_block, read_meta, registration_blocked,
    reuse_json_path, reuse_refusal, write_meta_strict,
};
use crate::app_state::{AppState, AppStateInner};
use crate::cold_start::RegistrationRun;

struct Request {
    reason: String,
    origin: String,
    pause: Duration,
    startup: bool,
}

struct Job {
    state: Weak<AppStateInner>,
    run: Arc<RegistrationRun>,
    finished: bool,
}

pub(super) fn start_startup(state: &AppState) {
    let _ = start(state, startup_request());
}

pub(super) async fn ensure(state: &AppState) {
    if let Some(run) = start(state, startup_request()) {
        let _ = run.first_result().await;
    }
}

pub(super) async fn request(
    state: &AppState,
    reason: &str,
    origin: &str,
    pause: Duration,
) -> Registration {
    let run = start(
        state,
        Request {
            reason: reason.into(),
            origin: origin.into(),
            pause,
            startup: false,
        },
    )
    .expect("显式注册请求总会取得任务");
    run.first_result().await
}

fn startup_request() -> Request {
    Request {
        reason: "启动自动注册".into(),
        origin: hongguo_api::register::REGISTER_ORIGIN.into(),
        pause: Duration::from_millis(REGISTER_RETRY_MS),
        startup: true,
    }
}

fn start(state: &AppState, request: Request) -> Option<Arc<RegistrationRun>> {
    let (run, claimed) = state.registration.claim(request.startup)?;
    if !claimed {
        if !request.startup {
            run.wake.notify_one();
        }
        return Some(run);
    }
    let job = Job {
        state: Arc::downgrade(state),
        run: run.clone(),
        finished: false,
    };
    let task = crate::runtime::spawn(run_registration(job, request));
    run.set_abort(task.abort_handle());
    Some(run)
}

async fn run_registration(mut job: Job, request: Request) {
    let Some(state) = job.state.upgrade() else {
        return;
    };
    if let Some(result) = prepare(&state, request.startup) {
        job.run.complete_first(result);
        job.finish();
        return;
    }
    drop(state);
    let material = RegisterMaterial::mint();
    for attempt in 1..=REGISTER_RETRY_MAX {
        let Some(state) = job.state.upgrade() else {
            return;
        };
        let mut status = state.registration.status();
        status.phase = RegistrationPhase::Registering;
        status.attempt = attempt;
        status.next_retry_at = None;
        state.registration.publish(status);
        let outcome = attempt_registration(&state, &request, &material).await;
        if publish_outcome(&state, &job.run, outcome, attempt, request.pause) {
            job.finish();
            return;
        }
        drop(state);
        tokio::select! {
            () = tokio::time::sleep(request.pause) => {},
            () = job.run.wake.notified() => {},
        }
    }
}

fn prepare(state: &AppState, startup: bool) -> Option<Registration> {
    if hongguo_common::offline_qa::enabled() {
        log::info!("[Device] 离线 QA：不注册、不打健康探针、不做行为预热");
        return Some(publish_refusal(state, "离线 QA 不发起注册"));
    }
    if let Some(refusal) = reuse_refusal(state) {
        let mut status = blocked_status(state, 0);
        status.set_failure(AppError::Rejected(
            refusal.error.clone().unwrap_or_default(),
        ));
        state.registration.publish(status);
        return Some(refusal);
    }
    let meta = read_meta(&state.store);
    if startup && hongguo_api::register::has_registered_ids(&state.device()) {
        if registration_blocked(&meta, now_ms()) {
            let _ = publish_refusal(state, "注册受限，继续复用已有设备身份");
        } else {
            state.registration.publish(RegistrationStatus {
                phase: RegistrationPhase::Ready,
                ..RegistrationStatus::default()
            });
        }
        return Some(ready(state));
    }
    if registration_blocked(&meta, now_ms()) {
        return Some(publish_refusal(
            state,
            "无可信持久状态或限制仍有效，不自动注册",
        ));
    }
    None
}

enum Outcome {
    Activated(Registration),
    Retry(AppError),
    Failed(AppError),
    Blocked(AppError),
}

async fn attempt_registration(
    state: &AppState,
    request: &Request,
    material: &RegisterMaterial,
) -> Outcome {
    let mut meta = read_meta(&state.store);
    if registration_blocked(&meta, now_ms()) {
        return Outcome::Blocked(AppError::Rejected("注册限制仍有效".into()));
    }
    if hongguo_common::offline_qa::enabled() || reuse_json_path().is_some() {
        return Outcome::Blocked(AppError::Rejected("当前模式禁止注册".into()));
    }
    meta.last_register_attempt_ms = Some(now_ms());
    if let Err(error) = write_meta_strict(&state.store, &meta) {
        return Outcome::Failed(error);
    }
    let env = ApiEnv {
        proxy: state.settings().proxy,
        device: anonymous_profile(material.cdid()),
        cookie: None,
        x_tt_token: None,
    };
    let started = Instant::now();
    let result =
        hongguo_api::register::register_material(&env, material, &request.origin, now_ms() as u64)
            .await;
    log::info!(
        "[Device] stage=registration elapsedMs={}",
        started.elapsed().as_millis()
    );
    match result {
        Ok(registration) => Outcome::Activated(activate_registration(
            state,
            &mut meta,
            &registration,
            &request.reason,
        )),
        Err(RegisterAttemptError::Blocked(block)) => {
            meta.last_register_error = Some("注册被拒绝或限流".into());
            if let Err(error) = persist_block(state, &mut meta, block) {
                REGISTER_FORCE_BLOCK.store(true, std::sync::atomic::Ordering::Relaxed);
                return Outcome::Failed(error);
            }
            Outcome::Blocked(AppError::Rejected("注册被拒绝或限流".into()))
        }
        Err(error) => {
            let retry = matches!(error, RegisterAttemptError::Transient(_));
            let error = AppError::from(error);
            meta.last_register_error = Some(error.to_string());
            if let Err(error) = write_meta_strict(&state.store, &meta) {
                return Outcome::Failed(error);
            }
            if retry {
                Outcome::Retry(error)
            } else {
                Outcome::Failed(error)
            }
        }
    }
}

fn publish_outcome(
    state: &AppState,
    run: &RegistrationRun,
    outcome: Outcome,
    attempt: usize,
    pause: Duration,
) -> bool {
    let (error, terminal, blocked) = match outcome {
        Outcome::Activated(result) if result.ok => {
            state.registration.publish(RegistrationStatus {
                phase: RegistrationPhase::Ready,
                attempt,
                ..RegistrationStatus::default()
            });
            run.complete_first(result);
            return true;
        }
        Outcome::Activated(result) => (
            AppError::Rejected(result.error.unwrap_or_else(|| "设备档案激活失败".into())),
            true,
            false,
        ),
        Outcome::Retry(error) => (error, attempt == REGISTER_RETRY_MAX, false),
        Outcome::Failed(error) => (error, true, false),
        Outcome::Blocked(error) => (error, true, true),
    };
    let mut status = if blocked {
        blocked_status(state, attempt)
    } else {
        RegistrationStatus {
            phase: if terminal {
                RegistrationPhase::Failed
            } else {
                RegistrationPhase::RetryScheduled
            },
            attempt,
            next_retry_at: (!terminal).then(|| {
                now_ms().saturating_add(i64::try_from(pause.as_millis()).unwrap_or(i64::MAX))
            }),
            ..RegistrationStatus::default()
        }
    };
    if terminal {
        log::error!("[Device] stage=registration attempt={attempt} error={error}");
    } else {
        log::warn!(
            "[Device] stage=registration attempt={attempt} retryScheduled=true error={error}"
        );
    }
    status.set_failure(error.clone());
    state.registration.publish(status);
    run.complete_first(failed(state, error));
    terminal
}

fn blocked_status(state: &AppState, attempt: usize) -> RegistrationStatus {
    let meta = read_meta(&state.store);
    RegistrationStatus {
        phase: RegistrationPhase::Blocked,
        attempt,
        block_until: meta.register_block_until_ms,
        is_manual_block: meta.register_block_manual || meta.register_block_unknown,
        ..RegistrationStatus::default()
    }
}

fn publish_refusal(state: &AppState, message: &str) -> Registration {
    let error = AppError::Rejected(message.into());
    let mut status = blocked_status(state, 0);
    status.set_failure(error.clone());
    state.registration.publish(status);
    log::warn!("[Device] stage=registration blocked=true error={error}");
    failed(state, error)
}

fn failed(state: &AppState, error: AppError) -> Registration {
    Registration {
        ok: false,
        iid: state.device().get("iid").into(),
        device_id: String::new(),
        kind: "error",
        error: Some(error.to_string()),
        network: matches!(error, AppError::Network(_)),
    }
}

fn ready(state: &AppState) -> Registration {
    let device = state.device();
    Registration {
        ok: true,
        iid: device.get("iid").into(),
        device_id: device.get("device_id").into(),
        kind: "registered",
        error: None,
        network: false,
    }
}

impl Job {
    fn finish(&mut self) {
        self.finished = true;
        if let Some(state) = self.state.upgrade() {
            state.registration.finish(&self.run);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let error = AppError::Internal("设备注册任务已中断".into());
        let mut status = state.registration.status();
        status.phase = RegistrationPhase::Failed;
        status.next_retry_at = None;
        status.set_failure(error.clone());
        state.registration.publish(status);
        self.run.complete_first(failed(&state, error));
        state.registration.finish(&self.run);
    }
}

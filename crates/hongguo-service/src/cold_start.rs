use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hongguo_common::{AppError, AppResult};
use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::{Notify, watch};
use tokio::task::AbortHandle;

use crate::app_state::AppState;
use crate::device::Registration;

static STARTUP_REQUIRED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RegistrationPhase {
    Idle,
    Registering,
    Ready,
    RetryScheduled,
    Blocked,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationStatus {
    pub phase: RegistrationPhase,
    pub attempt: usize,
    pub next_retry_at: Option<i64>,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
    pub block_until: Option<i64>,
    pub is_manual_block: bool,
    #[serde(skip)]
    pub(crate) failure: Option<AppError>,
}

impl Default for RegistrationStatus {
    fn default() -> Self {
        Self {
            phase: RegistrationPhase::Idle,
            attempt: 0,
            next_retry_at: None,
            error_kind: None,
            error_message: None,
            block_until: None,
            is_manual_block: false,
            failure: None,
        }
    }
}

impl RegistrationStatus {
    pub(crate) fn set_failure(&mut self, error: AppError) {
        self.error_kind = Some(error.i18n_key().into());
        self.error_message = Some(error.to_string());
        self.failure = Some(error);
    }
}

pub(crate) struct RegistrationRun {
    first: watch::Sender<Option<Registration>>,
    abort: Mutex<Option<AbortHandle>>,
    pub wake: Notify,
}

impl RegistrationRun {
    fn new() -> Self {
        let (first, _) = watch::channel(None);
        Self {
            first,
            abort: Mutex::new(None),
            wake: Notify::new(),
        }
    }

    pub fn set_abort(&self, abort: AbortHandle) {
        *self.abort.lock() = Some(abort);
    }

    pub fn complete_first(&self, result: Registration) {
        self.first.send_if_modified(|first| {
            if first.is_some() {
                return false;
            }
            *first = Some(result);
            true
        });
    }

    pub async fn first_result(&self) -> Registration {
        let mut changes = self.first.subscribe();
        loop {
            if let Some(result) = changes.borrow_and_update().clone() {
                return result;
            }
            if changes.changed().await.is_err() {
                unreachable!("注册任务持有首轮结果发送端");
            }
        }
    }
}

pub(crate) struct RegistrationCoordinator {
    status: watch::Sender<RegistrationStatus>,
    active: Mutex<Option<Arc<RegistrationRun>>>,
}

impl Default for RegistrationCoordinator {
    fn default() -> Self {
        let (status, _) = watch::channel(RegistrationStatus::default());
        Self {
            status,
            active: Mutex::new(None),
        }
    }
}

impl RegistrationCoordinator {
    pub fn status(&self) -> RegistrationStatus {
        self.status.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<RegistrationStatus> {
        self.status.subscribe()
    }

    pub fn publish(&self, status: RegistrationStatus) {
        self.status.send_replace(status);
    }

    pub fn claim(&self, startup: bool) -> Option<(Arc<RegistrationRun>, bool)> {
        let mut active = self.active.lock();
        if let Some(run) = active.as_ref() {
            return Some((run.clone(), false));
        }
        if startup && self.status().phase != RegistrationPhase::Idle {
            return None;
        }
        let run = Arc::new(RegistrationRun::new());
        *active = Some(run.clone());
        self.publish(RegistrationStatus {
            phase: RegistrationPhase::Registering,
            ..RegistrationStatus::default()
        });
        Some((run, true))
    }

    pub fn finish(&self, run: &Arc<RegistrationRun>) {
        let mut active = self.active.lock();
        if active
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, run))
        {
            *active = None;
        }
    }
}

impl Drop for RegistrationCoordinator {
    fn drop(&mut self) {
        if let Some(run) = self.active.get_mut().take()
            && let Some(abort) = run.abort.lock().take()
        {
            abort.abort();
        }
    }
}

/// 保留无参数的壳启动入口；实际任务和结果归属于各自 AppState。
pub fn arm() {
    STARTUP_REQUIRED.store(true, Ordering::Release);
}

pub async fn wait(state: &AppState) -> AppResult<()> {
    if hongguo_api::register::has_registered_ids(&state.device()) {
        return Ok(());
    }
    if STARTUP_REQUIRED.load(Ordering::Acquire) {
        crate::device::start_startup_registration(state);
    }
    let mut changes = state.registration.subscribe();
    loop {
        let status = changes.borrow_and_update().clone();
        if hongguo_api::register::has_registered_ids(&state.device()) {
            return Ok(());
        }
        if let Some(error) = status.failure {
            return Err(error);
        }
        match status.phase {
            RegistrationPhase::Idle => return Ok(()),
            RegistrationPhase::Ready => {
                return Err(AppError::InvalidArgs("设备注册身份不可用".into()));
            }
            RegistrationPhase::Registering => {}
            _ => return Err(AppError::Internal("设备注册状态缺少失败原因".into())),
        }
        changes
            .changed()
            .await
            .map_err(|_| AppError::Internal("设备注册状态通道已关闭".into()))?;
    }
}

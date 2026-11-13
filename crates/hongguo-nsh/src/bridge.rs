use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use hongguo_common::{AppError, AppResult};
use tokio::task::JoinHandle;

pub(crate) struct Request<T>(JoinHandle<AppResult<T>>);

#[derive(Debug, Clone)]
pub(crate) struct Failure {
    key: &'static str,
    message: String,
}

impl Failure {
    pub(crate) fn text(&self) -> nsh_core::element::SharedString {
        let translated = crate::i18n::tr(self.key);
        if translated.as_str() == self.key {
            self.message.clone().into()
        } else {
            translated
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.key)
    }
}

impl From<AppError> for Failure {
    fn from(error: AppError) -> Self {
        log::error!("[NSH request] {}: {error}", error.i18n_key());
        Self {
            key: error.i18n_key(),
            message: error.to_string(),
        }
    }
}

pub(crate) fn request<T, F>(future: F) -> Request<T>
where
    T: Send + 'static,
    F: Future<Output = AppResult<T>> + Send + 'static,
{
    Request(hongguo_service::runtime::spawn(future))
}

impl<T> Future for Request<T> {
    type Output = Result<T, Failure>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx).map(|result| {
            result
                .unwrap_or_else(|error| Err(AppError::Internal(format!("NSH task: {error}"))))
                .map_err(Failure::from)
        })
    }
}

impl<T> Drop for Request<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsh_core::prelude::*;

    #[component]
    fn ErrorNotice() -> impl IntoElement {
        let failure = Failure::from(AppError::Auth("Verification code expired".into()));
        text(failure.text())
    }

    #[test]
    fn transparent_errors_keep_the_actionable_service_message() {
        let mut app =
            nsh_testing::Headless::start(nsh_engine::EngineConfig::new(nsh_engine::Root::of::<
                ErrorNotice,
            >()))
            .unwrap();
        app.wait_for("Verification code expired").unwrap();
    }

    struct Finished(Option<tokio::sync::oneshot::Sender<()>>);
    impl Drop for Finished {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[tokio::test]
    async fn dropping_a_request_cancels_its_service_task() {
        let (started, ready) = tokio::sync::oneshot::channel();
        let (finished, stopped) = tokio::sync::oneshot::channel();
        let pending = request(async move {
            let _finished = Finished(Some(finished));
            started.send(()).unwrap();
            std::future::pending::<AppResult<()>>().await
        });
        ready.await.unwrap();
        drop(pending);
        tokio::time::timeout(std::time::Duration::from_secs(1), stopped)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn service_errors_are_returned_to_the_view() {
        let error = request(async { Err::<(), _>(AppError::InvalidArgs("test input".into())) })
            .await
            .unwrap_err();
        assert_eq!(error.key, "error.invalidArgs");
    }
}

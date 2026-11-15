use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use hongguo_common::{AppError, AppResult};
use tokio::task::JoinHandle;

pub(crate) struct Request<T>(JoinHandle<AppResult<T>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failure {
    key: &'static str,
    message: String,
    retryable: bool,
}

impl Failure {
    pub(crate) fn can_retry(&self) -> bool {
        self.retryable
    }
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
            retryable: !matches!(error, AppError::CodecUnsupported(_)),
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

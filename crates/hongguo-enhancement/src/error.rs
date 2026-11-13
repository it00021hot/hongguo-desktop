use std::fmt;

#[derive(Debug)]
pub enum Error {
    InvalidSize,
    InputTooLarge,
    InvalidStrength,
    Model(String),
    Gpu(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSize => f.write_str("invalid video dimensions"),
            Self::InputTooLarge => f.write_str("Anime4K input exceeds 1080p"),
            Self::InvalidStrength => f.write_str("enhancement strength is outside 0..=100"),
            Self::Model(detail) => write!(f, "Anime4K model: {detail}"),
            Self::Gpu(detail) => write!(f, "enhancement GPU: {detail}"),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) fn checked<T>(
    device: &wgpu::Device,
    operation: impl FnOnce() -> Result<T, Error>,
) -> impl Future<Output = Result<T, Error>> {
    let scopes = [
        device.push_error_scope(wgpu::ErrorFilter::OutOfMemory),
        device.push_error_scope(wgpu::ErrorFilter::Internal),
        device.push_error_scope(wgpu::ErrorFilter::Validation),
    ];
    let result = operation();
    let pending = scopes
        .into_iter()
        .rev()
        .map(wgpu::ErrorScopeGuard::pop)
        .collect::<Vec<_>>();
    async move {
        let mut error = None;
        for scope in pending {
            if let Some(failure) = scope.await {
                error.get_or_insert_with(|| Error::Gpu(failure.to_string()));
            }
        }
        match error {
            Some(error) => Err(error),
            None => result,
        }
    }
}

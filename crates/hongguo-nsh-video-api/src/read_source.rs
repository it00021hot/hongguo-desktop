//! Process-local, bounded random-access sources for native media readers.
//!
//! Registration owns visibility; each opened reader owns its cancellation. Providers
//! must periodically inspect cancellation while waiting for committed bytes.
use std::collections::HashMap;
use std::fmt;
use std::sync::{
    Arc, Mutex, OnceLock, Weak,
    atomic::{AtomicBool, Ordering},
};

pub const MAX_READ_BYTES: usize = 64 * 1024;
const SCHEME: &str = "hongguo-read";

#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    Invalid(String),
    DuplicateToken,
    UnknownSource,
    Revoked,
    Cancelled,
    Io(String),
}
impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) | Self::Io(message) => f.write_str(message),
            Self::DuplicateToken => f.write_str("media source token already registered"),
            Self::UnknownSource => f.write_str("media source is unavailable"),
            Self::Revoked => f.write_str("media source was revoked"),
            Self::Cancelled => f.write_str("media reader was closed"),
        }
    }
}
impl std::error::Error for ReadError {}

#[derive(Debug, PartialEq, Eq)]
pub enum ReadResult {
    Data(Vec<u8>),
    Eof,
}

pub trait ReadSource: Send + Sync {
    /// Unknown until the producer has committed its complete output.
    fn size(&self) -> Result<Option<u64>, ReadError>;
    /// Return a nonempty partial read or real final EOF; never a temporary EOF.
    fn read_at(
        &self,
        offset: u64,
        limit: usize,
        cancel: &ReadCancellation,
    ) -> Result<ReadResult, ReadError>;
}

struct Registered {
    source: Arc<dyn ReadSource>,
    revoked: Arc<AtomicBool>,
}
type Registry = Mutex<HashMap<String, Weak<Registered>>>;
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::default)
}
fn lock_registry() -> std::sync::MutexGuard<'static, HashMap<String, Weak<Registered>>> {
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct ReadRegistration {
    token: String,
    url: String,
    registered: Arc<Registered>,
}
impl ReadRegistration {
    pub fn url(&self) -> &str {
        &self.url
    }
}
impl Drop for ReadRegistration {
    fn drop(&mut self) {
        self.registered.revoked.store(true, Ordering::Release);
        lock_registry().remove(&self.token);
    }
}

fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 128
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

pub fn register_source(
    token: &str,
    source: Arc<dyn ReadSource>,
) -> Result<ReadRegistration, ReadError> {
    if !valid_token(token) {
        return Err(ReadError::Invalid("invalid media source token".into()));
    }
    let registered = Arc::new(Registered {
        source,
        revoked: Arc::new(AtomicBool::new(false)),
    });
    let mut registry = lock_registry();
    if registry.get(token).and_then(Weak::upgrade).is_some() {
        return Err(ReadError::DuplicateToken);
    }
    registry.insert(token.to_owned(), Arc::downgrade(&registered));
    Ok(ReadRegistration {
        token: token.to_owned(),
        url: format!("{SCHEME}://{token}"),
        registered,
    })
}

/// Recognizes the reserved scheme even when its remainder is malformed.
pub fn is_read_source_url(url: &str) -> bool {
    url.split_once(':')
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case(SCHEME))
}

pub fn open_source(url: &str) -> Result<ReadLease, ReadError> {
    let token = url
        .split_once("://")
        .filter(|(scheme, token)| scheme.eq_ignore_ascii_case(SCHEME) && valid_token(token))
        .map(|(_, token)| token)
        .ok_or_else(|| ReadError::Invalid("invalid media source URL".into()))?;
    let registered = lock_registry()
        .get(token)
        .and_then(Weak::upgrade)
        .ok_or(ReadError::UnknownSource)?;
    let cancel = ReadCancellation {
        revoked: registered.revoked.clone(),
        closed: AtomicBool::new(false),
    };
    cancel.check()?;
    Ok(ReadLease { registered, cancel })
}

pub struct ReadCancellation {
    revoked: Arc<AtomicBool>,
    closed: AtomicBool,
}
impl ReadCancellation {
    pub fn is_cancelled(&self) -> bool {
        self.check().is_err()
    }
    fn check(&self) -> Result<(), ReadError> {
        if self.revoked.load(Ordering::Acquire) {
            Err(ReadError::Revoked)
        } else if self.closed.load(Ordering::Acquire) {
            Err(ReadError::Cancelled)
        } else {
            Ok(())
        }
    }
}

pub struct ReadLease {
    registered: Arc<Registered>,
    cancel: ReadCancellation,
}
impl ReadLease {
    pub fn close(&self) {
        self.cancel.closed.store(true, Ordering::Release);
    }
    pub fn size(&self) -> Result<Option<u64>, ReadError> {
        self.cancel.check()?;
        let result = self.registered.source.size();
        self.cancel.check()?;
        result
    }
    pub fn read_at(&self, offset: u64, limit: usize) -> Result<ReadResult, ReadError> {
        self.cancel.check()?;
        let limit = limit.min(MAX_READ_BYTES);
        if limit == 0 || offset.checked_add(limit as u64).is_none() {
            return Err(ReadError::Invalid("invalid media read range".into()));
        }
        let result = self.registered.source.read_at(offset, limit, &self.cancel);
        self.cancel.check()?;
        let result = result?;
        match &result {
            ReadResult::Data(bytes) if bytes.is_empty() || bytes.len() > limit => {
                return Err(ReadError::Invalid(
                    "invalid media source read length".into(),
                ));
            }
            ReadResult::Eof if self.size()?.is_none_or(|length| offset < length) => {
                return Err(ReadError::Invalid(
                    "media source returned premature EOF".into(),
                ));
            }
            _ => {}
        }
        Ok(result)
    }
}
impl Drop for ReadLease {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests;

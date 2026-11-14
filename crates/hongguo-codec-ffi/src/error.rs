use std::ffi::{CStr, c_char};

use crate::ffi;

pub type CodecResult<T> = Result<T, CodecError>;

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("{operation}: {description} ({code})")]
    Native {
        operation: &'static str,
        code: i32,
        description: String,
    },
    #[error("{0}")]
    Invalid(&'static str),
}

pub(crate) fn check(code: i32, operation: &'static str) -> CodecResult<i32> {
    if code >= 0 {
        return Ok(code);
    }
    const ERROR_BUFFER_SIZE: usize = 256;
    let mut buffer = [0 as c_char; ERROR_BUFFER_SIZE];
    unsafe {
        ffi::hg_error_string(code, buffer.as_mut_ptr(), buffer.len());
    }
    let description = unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    Err(CodecError::Native {
        operation,
        code,
        description,
    })
}

pub(crate) fn packet_size(size: usize) -> CodecResult<i32> {
    i32::try_from(size).map_err(|_| CodecError::Invalid("packet exceeds native decoder size limit"))
}

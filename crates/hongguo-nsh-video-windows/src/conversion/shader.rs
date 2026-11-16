use std::ffi::CStr;
use windows::{
    Win32::Graphics::Direct3D::{
        Fxc::{D3DCOMPILE_OPTIMIZATION_LEVEL3, D3DCompile},
        ID3DBlob,
    },
    core::PCSTR,
};

use crate::VideoError;

pub(super) fn compile(entry: &CStr, target: &CStr) -> Result<ID3DBlob, VideoError> {
    let source = format!(
        "{}\n{}",
        hongguo_video_color::HLSL,
        include_str!("shader.hlsl")
    );
    let (mut output, mut diagnostic) = (None, None);
    // SAFETY: all compiler inputs are owned bytes or static C strings, and out pointers are valid.
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry.as_ptr().cast()),
            PCSTR(target.as_ptr().cast()),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut output,
            Some(&mut diagnostic),
        )
    };
    if let Err(error) = result {
        let detail = diagnostic
            .as_ref()
            .map(|blob: &ID3DBlob| {
                // SAFETY: the blob owns this diagnostic byte range.
                let bytes = unsafe {
                    std::slice::from_raw_parts(blob.GetBufferPointer().cast(), blob.GetBufferSize())
                };
                String::from_utf8_lossy(bytes)
                    .trim_end_matches('\0')
                    .to_owned()
            })
            .unwrap_or_else(|| error.to_string());
        return Err(super::error(detail));
    }
    output.ok_or_else(|| super::error("D3DCompile returned no shader bytecode"))
}

pub(super) fn bytes(blob: &ID3DBlob) -> &[u8] {
    // SAFETY: this borrowed immutable bytecode is valid for exactly the blob's lifetime.
    unsafe { std::slice::from_raw_parts(blob.GetBufferPointer().cast(), blob.GetBufferSize()) }
}

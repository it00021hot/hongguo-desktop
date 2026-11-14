//! Nonblocking GPU completion for a frame actually drawn by the window.
use crate::VideoError;
use hongguo_nsh_video_api::ErrorCode;
use windows::{
    Win32::Graphics::{
        Direct2D::ID2D1DeviceContext,
        Direct3D11::{
            D3D11_ASYNC_GETDATA_DONOTFLUSH, D3D11_QUERY_DESC, D3D11_QUERY_EVENT, ID3D11Device,
            ID3D11DeviceContext, ID3D11Query,
        },
    },
    core::BOOL,
};

pub(crate) struct Presentation {
    context: ID3D11DeviceContext,
    query: ID3D11Query,
    pub generation: u64,
    pub serial: u64,
}

impl Presentation {
    pub fn submit(
        dc: &ID2D1DeviceContext,
        device: &ID3D11Device,
        generation: u64,
        serial: u64,
    ) -> Result<Self, VideoError> {
        let pending = Self::prepare(device, generation, serial)?;
        // SAFETY: Flush submits DrawBitmap before the same-device EVENT query.
        unsafe { dc.Flush(None, None) }.map_err(error)?;
        pending.end();
        Ok(pending)
    }

    /// Allocate before recording foreign GPU work so an allocation failure cannot
    /// release a decoder pool lease while its copy is already in flight.
    pub fn prepare(
        device: &ID3D11Device,
        generation: u64,
        serial: u64,
    ) -> Result<Self, VideoError> {
        unsafe {
            let context = device.GetImmediateContext().map_err(error)?;
            let mut query = None;
            device
                .CreateQuery(
                    &D3D11_QUERY_DESC {
                        Query: D3D11_QUERY_EVENT,
                        MiscFlags: 0,
                    },
                    Some(&mut query),
                )
                .map_err(error)?;
            let query = query.ok_or_else(|| error("D3D11 returned no presentation query"))?;
            Ok(Self {
                context,
                query,
                generation,
                serial,
            })
        }
    }

    /// All fallible work precedes submission. Commands use the protected immediate context.
    pub fn end(&self) {
        unsafe {
            self.context.End(&self.query);
            self.context.Flush();
        }
    }

    pub fn completed(&self) -> Result<bool, VideoError> {
        let mut ready = BOOL(0);
        // SAFETY: EVENT queries write one BOOL. S_FALSE leaves our false value
        // unchanged; only an actual completed query may acknowledge presentation.
        unsafe {
            self.context
                .GetData(
                    &self.query,
                    Some((&raw mut ready).cast()),
                    std::mem::size_of::<BOOL>() as u32,
                    D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
                )
                .map_err(error)?;
        }
        Ok(ready.as_bool())
    }
}

fn error(error: impl std::fmt::Display) -> VideoError {
    VideoError::new(ErrorCode::FrameUnavailable, error.to_string())
}

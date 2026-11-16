use super::*;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows::core::HRESULT;

const BUSY_BUDGET: Duration = Duration::from_millis(250);
const BUSY_POLL: Duration = Duration::from_millis(1);
const D3DERR_WAS_STILL_DRAWING: HRESULT = HRESULT(0x8876_021c_u32 as i32);
const E_PENDING: HRESULT = HRESULT(0x8000_000a_u32 as i32);

struct Guard<'a>(&'a ID3D11Multithread);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        unsafe { self.0.Leave() };
    }
}

impl Decoder {
    pub(super) fn submit(
        &self,
        view: &ID3D11VideoDecoderOutputView,
        picture: &Picture,
        parameters: &[u8],
    ) -> Result<(), String> {
        let Self {
            video,
            native: decoder,
            context,
            protection,
            cancelled,
            ..
        } = self;
        let _guard = begin(video, decoder, protection, view, cancelled)?;
        let submitted = (|| {
            let mut buffers = Vec::with_capacity(4);
            buffers.push(buffer(
                video,
                decoder,
                D3D11_VIDEO_DECODER_BUFFER_PICTURE_PARAMETERS,
                parameters,
            )?);
            if let Some(matrix) = picture.quantization() {
                buffers.push(buffer(
                    video,
                    decoder,
                    D3D11_VIDEO_DECODER_BUFFER_INVERSE_QUANTIZATION_MATRIX,
                    &matrix,
                )?);
            }
            let size = picture.bitstream.len().next_multiple_of(128);
            let mut bytes = picture.bitstream.clone();
            bytes.resize(size, 0);
            buffers.push(buffer(
                video,
                decoder,
                D3D11_VIDEO_DECODER_BUFFER_BITSTREAM,
                &bytes,
            )?);
            let slices = picture.slice_control(size).map_err(error)?;
            buffers.push(buffer(
                video,
                decoder,
                D3D11_VIDEO_DECODER_BUFFER_SLICE_CONTROL,
                &slices,
            )?);
            unsafe { video.SubmitDecoderBuffers(decoder, &buffers) }.map_err(error)
        })();
        let ended = unsafe { video.DecoderEndFrame(decoder) }.map_err(error);
        submitted?;
        ended?;
        unsafe { context.Flush() };
        Ok(())
    }
}

fn begin<'a>(
    video: &ID3D11VideoContext,
    decoder: &ID3D11VideoDecoder,
    protection: &'a ID3D11Multithread,
    view: &ID3D11VideoDecoderOutputView,
    cancelled: &AtomicBool,
) -> Result<Guard<'a>, String> {
    let started = Instant::now();
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("HEVC hardware submission cancelled".into());
        }
        unsafe { protection.Enter() };
        let guard = Guard(protection);
        match unsafe { video.DecoderBeginFrame(decoder, view, 0, None) } {
            Ok(()) => return Ok(guard),
            Err(failure) => {
                drop(guard);
                let busy = (failure.code() == E_PENDING
                    || failure.code() == D3DERR_WAS_STILL_DRAWING)
                    || failure.code()
                        == windows::Win32::Graphics::Dxgi::DXGI_ERROR_WAS_STILL_DRAWING;
                if !busy {
                    return Err(error(failure));
                }
                if started.elapsed() >= BUSY_BUDGET {
                    return Err("HEVC hardware remained busy beyond submission budget".into());
                }
                std::thread::sleep(BUSY_POLL);
            }
        }
    }
}

fn buffer(
    video: &ID3D11VideoContext,
    decoder: &ID3D11VideoDecoder,
    kind: D3D11_VIDEO_DECODER_BUFFER_TYPE,
    data: &[u8],
) -> Result<D3D11_VIDEO_DECODER_BUFFER_DESC, String> {
    let (mut capacity, mut pointer) = (0, std::ptr::null_mut());
    unsafe { video.GetDecoderBuffer(decoder, kind, &mut capacity, &mut pointer) }.map_err(error)?;
    let copied = if data.len() > capacity as usize || pointer.is_null() {
        Err("HEVC access unit exceeds the driver decoder buffer".to_string())
    } else {
        // SAFETY: the driver mapped at least data.len() writable bytes, released below on every path.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), pointer.cast(), data.len()) };
        Ok(())
    };
    let released = unsafe { video.ReleaseDecoderBuffer(decoder, kind) }.map_err(error);
    copied?;
    released?;
    Ok(D3D11_VIDEO_DECODER_BUFFER_DESC {
        BufferType: kind,
        DataSize: data.len() as u32,
        ..Default::default()
    })
}

//! Two immutable published output slots. A slot is reused only after the final
//! displayed-frame reference drops; D2D Flush precedes the next D3D11 fence/copy.

use crate::{FrameLease, SharedFrames, VideoError, draw};
use hongguo_nsh_video_api::{ErrorCode, ResizeMode};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use windows::Win32::Graphics::{
    Direct2D::{ID2D1Bitmap1, ID2D1DeviceContext},
    Direct3D11::{ID3D11Device, ID3D11Texture2D},
};

pub(crate) struct OutputSlot {
    shared: Mutex<SharedFrames>,
    texture: ID3D11Texture2D,
    size: (u32, u32),
    busy: AtomicBool,
}

impl OutputSlot {
    pub fn new(
        device: Arc<wgpu::Device>,
        decoder: &ID3D11Device,
        width: u32,
        height: u32,
    ) -> Result<Arc<Self>, VideoError> {
        let shared = SharedFrames::new(device, decoder, width, height)?;
        let texture = shared.native_texture().clone();
        Ok(Arc::new(Self {
            shared: Mutex::new(shared),
            texture,
            size: (width, height),
            busy: AtomicBool::new(false),
        }))
    }

    pub fn acquire(self: &Arc<Self>) -> Result<Option<OutputLease>, VideoError> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(None);
        }
        let result = self
            .shared
            .lock()
            .map_err(|error| VideoError::new(ErrorCode::GpuFailed, error.to_string()))?
            .blank();
        match result {
            Ok(frame) => Ok(Some(OutputLease {
                slot: self.clone(),
                frame,
            })),
            Err(error) => {
                self.busy.store(false, Ordering::Release);
                Err(error)
            }
        }
    }
}

pub(crate) struct OutputLease {
    slot: Arc<OutputSlot>,
    frame: FrameLease,
}

impl OutputLease {
    pub fn texture(&self) -> &wgpu::Texture {
        self.frame.texture()
    }

    pub fn submit(
        self,
        input: FrameLease,
        mut encoder: wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        complete: impl FnOnce(Arc<DisplayedFrame>) + Send + 'static,
    ) {
        input.release(&mut encoder);
        self.frame.release(&mut encoder);
        queue.submit([encoder.finish()]);
        let slot = self.slot;
        let frame = self.frame;
        queue.on_submitted_work_done(move || {
            input.completed();
            frame.completed();
            drop(input);
            drop(frame);
            complete(Arc::new(DisplayedFrame { slot }));
        });
    }

    pub fn discard(self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let mut encoder = device.create_command_encoder(&Default::default());
        self.frame.release(&mut encoder);
        queue.submit([encoder.finish()]);
        let slot = self.slot;
        self.frame
            .retire(queue, move || slot.busy.store(false, Ordering::Release));
    }
}

pub struct DisplayedFrame {
    slot: Arc<OutputSlot>,
}

impl DisplayedFrame {
    pub fn same_slot(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.slot, &other.slot)
    }
    pub fn bitmap(&self, dc: &ID2D1DeviceContext) -> Result<ID2D1Bitmap1, VideoError> {
        draw::bitmap(dc, &self.slot.texture)
    }

    pub fn draw(
        &self,
        dc: &ID2D1DeviceContext,
        bitmap: &ID2D1Bitmap1,
        target: (f32, f32),
        mode: ResizeMode,
    ) -> Result<(), VideoError> {
        draw::draw_bitmap(dc, bitmap, self.slot.size, target, mode)?;
        // SAFETY: Flush submits all reads of this shared frame before its reference
        // may be dropped and a later D3D11 output clear/fence is queued.
        unsafe { dc.Flush(None, None) }
            .map_err(|error| VideoError::new(ErrorCode::GpuFailed, error.to_string()))
    }
}

impl Drop for DisplayedFrame {
    fn drop(&mut self) {
        self.slot.busy.store(false, Ordering::Release);
    }
}

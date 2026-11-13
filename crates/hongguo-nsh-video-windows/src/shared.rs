//! Same-adapter D3D11 soft-decoded BGRA → DX12/WGPU input frames.
//! Native registration and Direct2D output are separate from this interop boundary.

use hongguo_nsh_video_api::{ErrorCode, FrameMetadata};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, GENERIC_ALL, HANDLE},
        Graphics::{
            Direct3D11::{
                ID3D11Device, ID3D11Device1, ID3D11Device5, ID3D11DeviceContext4, ID3D11Fence,
                ID3D11Texture2D,
            },
            Direct3D12::{
                D3D12_FENCE_FLAG_SHARED, D3D12_HEAP_FLAG_SHARED, D3D12_HEAP_PROPERTIES,
                D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
                D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS, D3D12_RESOURCE_STATE_COMMON,
                D3D12_TEXTURE_LAYOUT_UNKNOWN, ID3D12Fence, ID3D12Resource,
            },
            Dxgi::{
                Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC},
                IDXGIDevice,
            },
        },
    },
    core::Interface,
};

#[derive(Clone, Debug)]
pub struct VideoError {
    code: ErrorCode,
    detail: String,
}

impl VideoError {
    pub(crate) fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
    pub fn code(&self) -> ErrorCode {
        self.code
    }
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for VideoError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str(&self.detail)
    }
}
impl std::error::Error for VideoError {}

fn gpu_error(error: impl std::fmt::Display) -> VideoError {
    VideoError {
        code: ErrorCode::GpuUnavailable,
        detail: error.to_string(),
    }
}

struct SharedHandle(HANDLE);
impl Drop for SharedHandle {
    fn drop(&mut self) {
        // SAFETY: Only this RAII guard owns the successfully created NT handle.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub struct SharedFrames {
    device: Arc<wgpu::Device>,
    resource: ID3D12Resource,
    texture11: ID3D11Texture2D,
    context11: ID3D11DeviceContext4,
    fence11: ID3D11Fence,
    fence12: ID3D12Fence,
    serial: u64,
    size: wgpu::Extent3d,
    busy: Arc<AtomicBool>,
}

impl SharedFrames {
    /// `decoder` must be the video-enabled D3D11 device attached to this MediaEngine.
    /// The adapter identity is verified against WGPU before any resource is shared.
    pub fn new(
        device: Arc<wgpu::Device>,
        decoder: &ID3D11Device,
        width: u32,
        height: u32,
    ) -> Result<Self, VideoError> {
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return Err(gpu_error("invalid shared video dimensions"));
        }
        // SAFETY: This guard exposes only the live DX12 device owned by `device`.
        let native = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or_else(|| gpu_error("video bridge requires WGPU DX12"))?;
        let device12 = native.raw_device();
        // SAFETY: These retained COM interfaces are used with their declared methods and ownership.
        let (resource, texture11, context11, fence11, fence12) = unsafe {
            let dxgi: IDXGIDevice = decoder.cast().map_err(gpu_error)?;
            let description = dxgi
                .GetAdapter()
                .map_err(gpu_error)?
                .GetDesc()
                .map_err(gpu_error)?;
            let luid = device12.GetAdapterLuid();
            if description.AdapterLuid.LowPart != luid.LowPart
                || description.AdapterLuid.HighPart != luid.HighPart
            {
                return Err(gpu_error("decoder and WGPU use different GPU adapters"));
            }
            let heap = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                CreationNodeMask: 1,
                VisibleNodeMask: 1,
                ..Default::default()
            };
            let descriptor = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: u64::from(width),
                Height: height,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
                Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET
                    | D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
                ..Default::default()
            };
            let mut resource: Option<ID3D12Resource> = None;
            device12
                .CreateCommittedResource(
                    &heap,
                    D3D12_HEAP_FLAG_SHARED,
                    &descriptor,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut resource,
                )
                .map_err(gpu_error)?;
            let resource =
                resource.ok_or_else(|| gpu_error("DX12 returned no shared video resource"))?;
            let handle = SharedHandle(
                device12
                    .CreateSharedHandle(&resource, None, GENERIC_ALL.0, None)
                    .map_err(gpu_error)?,
            );
            let device1: ID3D11Device1 = decoder.cast().map_err(gpu_error)?;
            let texture11: ID3D11Texture2D =
                device1.OpenSharedResource1(handle.0).map_err(gpu_error)?;
            let fence12: ID3D12Fence = device12
                .CreateFence(0, D3D12_FENCE_FLAG_SHARED)
                .map_err(gpu_error)?;
            let handle = SharedHandle(
                device12
                    .CreateSharedHandle(&fence12, None, GENERIC_ALL.0, None)
                    .map_err(gpu_error)?,
            );
            let device5: ID3D11Device5 = decoder.cast().map_err(gpu_error)?;
            let mut fence11 = None;
            device5
                .OpenSharedFence(handle.0, &mut fence11)
                .map_err(gpu_error)?;
            let fence11 = fence11.ok_or_else(|| gpu_error("D3D11 returned no shared GPU fence"))?;
            let context11: ID3D11DeviceContext4 = decoder
                .GetImmediateContext()
                .map_err(gpu_error)?
                .cast()
                .map_err(gpu_error)?;
            (resource, texture11, context11, fence11, fence12)
        };
        drop(native);
        Ok(Self {
            device,
            resource,
            texture11,
            context11,
            fence11,
            fence12,
            serial: 0,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            busy: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Original and enhanced output share the same retained soft-decoded frame.
    pub fn copy_frame(
        &mut self,
        raw: &ID3D11Texture2D,
        metadata: FrameMetadata,
    ) -> Result<Option<FrameLease>, VideoError> {
        if self.busy.load(Ordering::Acquire) {
            return Ok(None);
        }
        if (metadata.width, metadata.height) != (self.size.width, self.size.height) {
            return Err(gpu_error(
                "raw frame dimensions differ from the shared input",
            ));
        }
        // SAFETY: The source must be the fully initialized raw fallback from this
        // exact D3D11 device. Verify device/format before the GPU-only copy.
        unsafe {
            let source_device = raw.GetDevice().map_err(gpu_error)?;
            let destination_device = self.texture11.GetDevice().map_err(gpu_error)?;
            let mut description = Default::default();
            raw.GetDesc(&mut description);
            if source_device != destination_device
                || description.Format != DXGI_FORMAT_B8G8R8A8_UNORM
                || description.Width != self.size.width
                || description.Height != self.size.height
                || description.SampleDesc.Count != 1
            {
                return Err(gpu_error(
                    "raw fallback does not match the shared decoder texture",
                ));
            }
            self.context11.CopyResource(&self.texture11, raw);
        }
        self.finish_capture(metadata).map(Some)
    }

    fn finish_capture(&mut self, metadata: FrameMetadata) -> Result<FrameLease, VideoError> {
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or_else(|| gpu_error("video fence serial exhausted"))?;
        // SAFETY: The shared fence orders all D3D11 writes before WGPU reads.
        unsafe {
            self.context11
                .Signal(&self.fence11, self.serial)
                .map_err(gpu_error)?;
            self.context11.Flush();
            let native = self
                .device
                .as_hal::<wgpu::hal::api::Dx12>()
                .ok_or_else(|| gpu_error("DX12 device unavailable"))?;
            native
                .raw_queue()
                .Wait(&self.fence12, self.serial)
                .map_err(gpu_error)?;
        }
        // SAFETY: A completed/queued full-frame write initializes this shared resource before the GPU fence permits sampling.
        let texture = unsafe {
            let raw = wgpu::hal::dx12::Device::texture_from_raw(
                self.resource.clone(),
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureDimension::D2,
                self.size,
                1,
                1,
            );
            self.device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
                raw,
                &wgpu::TextureDescriptor {
                    label: Some("mediaengine-shared-frame"),
                    size: self.size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC
                        | wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                },
            )
        };
        self.busy.store(true, Ordering::Release);
        Ok(FrameLease {
            texture,
            metadata,
            busy: self.busy.clone(),
        })
    }

    pub(crate) fn blank(&mut self) -> Result<FrameLease, VideoError> {
        if self.busy.load(Ordering::Acquire) {
            return Err(gpu_error("shared output slot is still in flight"));
        }
        // SAFETY: An idle output slot has no readers. Clear initializes its full contents
        // before importing into WGPU, and orders after the previous D2D Flush on this device.
        unsafe {
            let device = self.texture11.GetDevice().map_err(gpu_error)?;
            let mut target = None;
            device
                .CreateRenderTargetView(&self.texture11, None, Some(&mut target))
                .map_err(gpu_error)?;
            let target =
                target.ok_or_else(|| gpu_error("D3D11 returned no shared output target"))?;
            self.context11
                .ClearRenderTargetView(&target, &[0.0, 0.0, 0.0, 1.0]);
        }
        let mut metadata = FrameMetadata::default();
        metadata.width = self.size.width;
        metadata.height = self.size.height;
        self.finish_capture(metadata)
    }

    pub(crate) fn native_texture(&self) -> &ID3D11Texture2D {
        &self.texture11
    }
}

/// Dropping a lease early deliberately leaves the frame slot busy. Recreate the bridge
/// on cancellation/device loss; never reuse a texture whose GPU completion is unknown.
pub struct FrameLease {
    texture: wgpu::Texture,
    metadata: FrameMetadata,
    busy: Arc<AtomicBool>,
}

impl FrameLease {
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
    pub fn metadata(&self) -> FrameMetadata {
        self.metadata
    }

    /// Submits all frame reads and the COMMON transition as one ordered batch.
    /// The caller must poll the same device to retire this lease before another capture.
    pub fn submit(
        self,
        mut encoder: wgpu::CommandEncoder,
        queue: &wgpu::Queue,
    ) -> wgpu::SubmissionIndex {
        // Native DX12 maps PRESENT to COMMON. The next external D3D11 writer owns it only after completion.
        self.release(&mut encoder);
        let submission = queue.submit([encoder.finish()]);
        self.retire(queue, || {});
        submission
    }

    pub(crate) fn release(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.transition_resources(
            std::iter::empty(),
            std::iter::once(wgpu::TextureTransition {
                texture: &self.texture,
                selector: None,
                state: wgpu::TextureUses::PRESENT,
            }),
        );
    }

    pub(crate) fn retire(self, queue: &wgpu::Queue, complete: impl FnOnce() + Send + 'static) {
        queue.on_submitted_work_done(move || {
            self.busy.store(false, Ordering::Release);
            drop(self);
            complete();
        });
    }

    pub(crate) fn completed(&self) {
        self.busy.store(false, Ordering::Release);
    }
}

use crate::OutputError;
use hongguo_nsh_video_api::{ErrorCode, FrameMetadata};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBuffer,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    kCVPixelFormatType_32BGRA,
};
use objc2_metal::{MTLPixelFormat, MTLTextureType};
use std::{ptr::NonNull, sync::Arc};

#[derive(Clone)]
pub struct DecodedFrame {
    buffer: CFRetained<CVPixelBuffer>,
    pts_ms: u64,
    width: u32,
    height: u32,
}

impl DecodedFrame {
    pub fn metadata(&self) -> FrameMetadata {
        let mut metadata = FrameMetadata::default();
        metadata.pts_ms = self.pts_ms;
        metadata.width = self.width;
        metadata.height = self.height;
        metadata
    }

    pub(crate) fn new(buffer: CFRetained<CVPixelBuffer>, pts_ms: u64) -> Result<Self, OutputError> {
        if CVPixelBufferGetPixelFormatType(&buffer) != kCVPixelFormatType_32BGRA {
            return Err(OutputError::new(
                ErrorCode::FrameFormatUnsupported,
                "expected decoded BGRA frame",
            ));
        }
        let width = u32::try_from(CVPixelBufferGetWidth(&buffer)).map_err(|_| {
            OutputError::new(
                ErrorCode::FrameFormatUnsupported,
                "decoded frame width exceeds u32",
            )
        })?;
        let height = u32::try_from(CVPixelBufferGetHeight(&buffer)).map_err(|_| {
            OutputError::new(
                ErrorCode::FrameFormatUnsupported,
                "decoded frame height exceeds u32",
            )
        })?;
        if width == 0 || height == 0 {
            return Err(OutputError::new(
                ErrorCode::FrameFormatUnsupported,
                "decoded frame is empty",
            ));
        }
        Ok(Self {
            buffer,
            pts_ms,
            width,
            height,
        })
    }
}

pub struct MetalFrames {
    device: Arc<wgpu::Device>,
    cache: CFRetained<CVMetalTextureCache>,
}

impl MetalFrames {
    pub fn new(device: Arc<wgpu::Device>) -> Result<Self, OutputError> {
        // SAFETY: The backend guard borrows this live device; no native device is replaced.
        let native = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }.ok_or_else(|| {
            OutputError::new(
                ErrorCode::GpuUnavailable,
                "Apple pixel bridge requires the Metal backend",
            )
        })?;
        let mut cache = std::ptr::null_mut();
        // SAFETY: CoreVideo creates a cache for the same Metal device that owns the wgpu queue.
        let result = unsafe {
            CVMetalTextureCache::create(
                None,
                None,
                native.raw_device(),
                None,
                NonNull::from(&mut cache),
            )
        };
        if result != 0 {
            return Err(OutputError::new(
                ErrorCode::GpuFailed,
                format!("CVMetalTextureCacheCreate returned {result}"),
            ));
        }
        let cache = NonNull::new(cache).ok_or_else(|| {
            OutputError::new(
                ErrorCode::GpuFailed,
                "CoreVideo returned no Metal texture cache",
            )
        })?;
        // SAFETY: CoreVideo Create returns a +1 retained reference.
        let cache = unsafe { CFRetained::from_raw(cache) };
        drop(native);
        Ok(Self { device, cache })
    }

    pub fn import(&self, frame: DecodedFrame) -> Result<ImportedFrame, OutputError> {
        let mut image = std::ptr::null_mut();
        // SAFETY: BGRA dimensions and format were checked; CoreVideo retains the backing IOSurface.
        let result = unsafe {
            CVMetalTextureCache::create_texture_from_image(
                None,
                &self.cache,
                &frame.buffer,
                None,
                MTLPixelFormat::BGRA8Unorm,
                frame.width as usize,
                frame.height as usize,
                0,
                NonNull::from(&mut image),
            )
        };
        if result != 0 {
            return Err(OutputError::new(
                ErrorCode::GpuFailed,
                format!("CVMetalTexture creation returned {result}"),
            ));
        }
        let image = NonNull::new(image).ok_or_else(|| {
            OutputError::new(ErrorCode::GpuFailed, "CoreVideo returned no Metal image")
        })?;
        // SAFETY: CoreVideo Create returns a +1 retained image, retained with the decoded frame.
        let image = unsafe { CFRetained::from_raw(image) };
        let native = CVMetalTextureGetTexture(&image).ok_or_else(|| {
            OutputError::new(ErrorCode::GpuFailed, "CoreVideo image has no Metal texture")
        })?;
        let size = wgpu::Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        };
        // SAFETY: The texture belongs to this cache's same Metal device and remains retained by ImportedFrame.
        let texture = unsafe {
            let native = wgpu::hal::metal::Device::texture_from_raw(
                native,
                wgpu::TextureFormat::Bgra8Unorm,
                MTLTextureType::Type2D,
                1,
                1,
                size.into(),
            );
            self.device
                .create_texture_from_hal::<wgpu::hal::api::Metal>(
                    native,
                    &wgpu::TextureDescriptor {
                        label: Some("decoded-video-frame"),
                        size,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Bgra8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                        view_formats: &[],
                    },
                )
        };
        Ok(ImportedFrame {
            texture,
            metadata: frame.metadata(),
            _frame: Some(frame),
            _image: Some(image),
        })
    }
}

/// Keep this owner alive until the queue has completed every submission reading its texture.
#[derive(Clone)]
pub struct ImportedFrame {
    texture: wgpu::Texture,
    metadata: FrameMetadata,
    _frame: Option<DecodedFrame>,
    _image: Option<CFRetained<CVMetalTexture>>,
}

// SAFETY: The pixel buffer and CVMetalTexture are immutable after import; their retain/release is thread-safe.
unsafe impl Send for ImportedFrame {}

impl ImportedFrame {
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    pub fn metadata(&self) -> FrameMetadata {
        self.metadata
    }

    pub(crate) fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &hongguo_nsh_video_api::frames::PixelFrame,
    ) -> Self {
        let size = wgpu::Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("softdec-bgra"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &frame.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.stride),
                rows_per_image: Some(frame.height),
            },
            size,
        );
        let mut metadata = FrameMetadata::default();
        metadata.width = frame.width;
        metadata.height = frame.height;
        metadata.pts_ms = frame.pts_ms;
        Self {
            texture,
            metadata,
            _frame: None,
            _image: None,
        }
    }

    /// Call after submitting all commands that read this frame; poll the device to release completed frames.
    pub fn retire_after_submitted_work(self, queue: &wgpu::Queue) {
        queue.on_submitted_work_done(move || drop(self));
    }
}

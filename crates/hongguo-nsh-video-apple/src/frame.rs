use hongguo_media_types::PresentationFrame;
use hongguo_nsh_video_api::{FrameMetadata, frames::PixelFrame};
use hongguo_video_gpu::ConvertedFrame;
use std::sync::Arc;

#[derive(Clone)]
pub struct ImportedFrame {
    texture: wgpu::Texture,
    metadata: FrameMetadata,
    native: Option<Arc<PresentationFrame>>,
    _converted: Option<Arc<ConvertedFrame>>,
    aspect_ratio: f32,
}

impl ImportedFrame {
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
    pub fn metadata(&self) -> FrameMetadata {
        self.metadata
    }
    pub fn aspect_ratio(&self) -> f32 {
        self.aspect_ratio
    }
    pub fn source_descriptor(&self) -> Option<hongguo_media_types::FrameDescriptor> {
        self.native.as_ref().map(|frame| frame.image.descriptor())
    }

    pub(crate) fn converted(frame: Arc<PresentationFrame>, converted: ConvertedFrame) -> Self {
        let descriptor = &converted.descriptor;
        let sar = descriptor.color.sample_aspect_ratio.unwrap_or((1, 1));
        let mut metadata = FrameMetadata::default();
        metadata.width = descriptor.width as u32;
        metadata.height = descriptor.height as u32;
        metadata.pts_ms = (frame.pts_secs.max(0.0) * 1000.0).round() as u64;
        metadata.generation = frame.generation;
        metadata.serial = frame.serial;
        metadata.sample_aspect_ratio = sar;
        let aspect_ratio =
            descriptor.width as f32 * sar.0 as f32 / (descriptor.height as f32 * sar.1 as f32);
        Self {
            texture: converted.texture.clone(),
            metadata,
            native: Some(frame),
            _converted: Some(Arc::new(converted)),
            aspect_ratio,
        }
    }

    pub(crate) fn upload(device: &wgpu::Device, queue: &wgpu::Queue, frame: &PixelFrame) -> Self {
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
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
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
            native: None,
            _converted: None,
            aspect_ratio: frame.width as f32 / frame.height as f32,
        }
    }
}

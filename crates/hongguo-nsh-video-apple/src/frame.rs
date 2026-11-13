use hongguo_nsh_video_api::{FrameMetadata, frames::PixelFrame};

#[derive(Clone)]
pub struct ImportedFrame {
    texture: wgpu::Texture,
    metadata: FrameMetadata,
}

impl ImportedFrame {
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
    pub fn metadata(&self) -> FrameMetadata {
        self.metadata
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
        Self { texture, metadata }
    }

    pub fn retire_after_submitted_work(self, queue: &wgpu::Queue) {
        queue.on_submitted_work_done(move || drop(self));
    }
}

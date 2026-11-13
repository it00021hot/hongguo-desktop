use hongguo_nsh_video_api::frames::PixelFrame;
use std::sync::Arc;

pub(crate) struct DirectTexture {
    pub texture: wgpu::Texture,
    pub frame: Option<Arc<PixelFrame>>,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

impl DirectTexture {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("android-direct-rgba"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        Self {
            texture,
            frame: None,
            rgba: Vec::new(),
            width,
            height,
        }
    }

    pub fn upload(&mut self, queue: &wgpu::Queue, frame: Arc<PixelFrame>) -> Result<bool, String> {
        if self
            .frame
            .as_ref()
            .is_some_and(|old| (old.generation, old.serial) == (frame.generation, frame.serial))
        {
            return Ok(false);
        }
        if (frame.width, frame.height) != (self.width, self.height) {
            return Err("direct frame dimensions changed".into());
        }
        // The shared contract is BGRA8; the GLES sampling texture is RGBA8.
        crate::pixel_bytes::swizzle(
            frame.width,
            frame.height,
            frame.stride,
            &frame.pixels,
            &mut self.rgba,
        )?;
        queue.write_texture(
            self.texture.as_image_copy(),
            &self.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.width * 4),
                rows_per_image: Some(frame.height),
            },
            wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );
        self.frame = Some(frame);
        Ok(true)
    }
}

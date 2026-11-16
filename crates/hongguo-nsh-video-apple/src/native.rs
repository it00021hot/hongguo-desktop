use hongguo_media_types::{DecodeDevice, FrameImage, HardwareKind, PresentationFrame};
use hongguo_video_gpu::YuvInput;
use objc2::runtime::ProtocolObject;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetIOSurface,
    CVPixelBufferGetPixelFormatType, CVPixelBufferGetPlaneCount, CVPixelBufferGetWidthOfPlane,
};
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLTextureDescriptor, MTLTextureType, MTLTextureUsage,
};
use std::sync::Arc;

pub(crate) struct AppleDevice;

impl AppleDevice {
    pub fn lease() -> Arc<dyn DecodeDevice> {
        Arc::new(Self)
    }
}

unsafe impl DecodeDevice for AppleDevice {
    fn kind(&self) -> HardwareKind {
        HardwareKind::VideoToolbox
    }
    fn device(&self) -> usize {
        0
    }
    fn generation(&self) -> u64 {
        1
    }
}

pub(crate) fn import(device: &wgpu::Device, frame: &PresentationFrame) -> Result<YuvInput, String> {
    let FrameImage::Hardware(native) = &frame.image else {
        return Err("expected a native hardware frame".into());
    };
    let handle = native.handle();
    if handle.kind != HardwareKind::VideoToolbox || handle.object == 0 {
        return Err("not a VideoToolbox pixel buffer".into());
    }
    let pixel_buffer = unsafe { &*(handle.object as *const CVPixelBuffer) };
    let format = CVPixelBufferGetPixelFormatType(pixel_buffer);
    let (luma, chroma, high_shift) = match format.to_be_bytes() {
        [b'4', b'2', b'0', b'v'] | [b'4', b'2', b'0', b'f'] => {
            (MTLPixelFormat::R8Uint, MTLPixelFormat::RG8Uint, 0)
        }
        [b'x', b'4', b'2', b'0'] | [b'x', b'f', b'2', b'0'] => {
            (MTLPixelFormat::R16Uint, MTLPixelFormat::RG16Uint, 6)
        }
        _ => {
            return Err(format!(
                "unsupported native VideoToolbox pixel format {format:#x}"
            ));
        }
    };
    if CVPixelBufferGetPlaneCount(pixel_buffer) != 2 {
        return Err("VideoToolbox returned a non-biplanar surface".into());
    }
    let surface = CVPixelBufferGetIOSurface(Some(pixel_buffer))
        .ok_or("VideoToolbox frame has no IOSurface")?;
    let metal =
        unsafe { device.as_hal::<wgpu::hal::api::Metal>() }.ok_or("video GPU is not Metal")?;
    let raw_device: &ProtocolObject<dyn MTLDevice> = metal.raw_device();
    let descriptor = native.descriptor();
    let mut textures = Vec::with_capacity(3);
    for (index, pixel_format) in [luma, chroma].into_iter().enumerate() {
        let width = CVPixelBufferGetWidthOfPlane(pixel_buffer, index);
        let height = CVPixelBufferGetHeightOfPlane(pixel_buffer, index);
        let native_description = MTLTextureDescriptor::new();
        native_description.setTextureType(MTLTextureType::Type2D);
        native_description.setPixelFormat(pixel_format);
        unsafe {
            native_description.setWidth(width);
            native_description.setHeight(height);
        }
        native_description.setUsage(MTLTextureUsage::ShaderRead);
        let raw = raw_device
            .newTextureWithDescriptor_iosurface_plane(&native_description, &surface, index)
            .ok_or("cannot import the IOSurface plane into Metal")?;
        let gpu_format = match (index, high_shift) {
            (0, 0) => wgpu::TextureFormat::R8Uint,
            (1, 0) => wgpu::TextureFormat::Rg8Uint,
            (0, _) => wgpu::TextureFormat::R16Uint,
            _ => wgpu::TextureFormat::Rg16Uint,
        };
        let size = wgpu::Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        };
        let native_texture = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                raw,
                gpu_format,
                MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width: size.width,
                    height: size.height,
                    depth: 1,
                },
            )
        };
        let texture = unsafe {
            device.create_texture_from_hal::<wgpu::hal::api::Metal>(
                native_texture,
                &wgpu::TextureDescriptor {
                    label: Some("videotoolbox-iosurface-plane"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: gpu_format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        textures.push(texture);
    }
    textures.push(textures[1].clone());
    Ok(YuvInput {
        planes: textures
            .try_into()
            .map_err(|_| "invalid IOSurface planes")?,
        descriptor,
        chroma_divisor: [2, 2],
        interleaved: true,
        high_shift,
        monochrome: false,
    })
}

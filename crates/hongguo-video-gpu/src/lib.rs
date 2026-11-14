use hongguo_media_types::{ChromaSampling, ColorMetadata, FrameDescriptor, NativeFrame};
use hongguo_video_color::{ColorTransform, DisplayTarget, LUT_HEIGHT, LUT_WIDTH};
use std::{borrow::Cow, sync::Arc};

const WORKGROUP: u32 = 8;
const GEOMETRY_SIZE: u64 = 48;

pub struct YuvInput {
    pub planes: [wgpu::Texture; 3],
    pub descriptor: FrameDescriptor,
    pub chroma_divisor: [u32; 2],
    pub interleaved: bool,
    pub high_shift: u32,
    pub monochrome: bool,
}

struct ColorResources {
    key: (ColorMetadata, u8, usize, DisplayTarget),
    params: wgpu::Buffer,
    lut: wgpu::Texture,
}

pub struct YuvConverter {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipeline: wgpu::ComputePipeline,
    mapping: wgpu::ComputePipeline,
    color: Option<ColorResources>,
}

pub struct ConvertedFrame {
    pub texture: wgpu::Texture,
    pub descriptor: FrameDescriptor,
    _input: YuvInput,
}

impl YuvConverter {
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        let source = format!(
            "{}\n{}",
            hongguo_video_color::WGSL,
            include_str!("convert.wgsl")
        );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native-yuv-color"),
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(source)),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("native-yuv-color"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let source = format!(
            "{}\n{}",
            hongguo_video_color::WGSL,
            include_str!("map.wgsl")
        );
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native-color-map"),
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(source)),
        });
        let mapping = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("native-color-map"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            device,
            queue,
            pipeline,
            mapping,
            color: None,
        }
    }

    pub fn upload(&self, frame: &NativeFrame) -> Result<YuvInput, String> {
        let expected = if frame.chroma == ChromaSampling::Monochrome {
            1
        } else {
            3
        };
        if frame.planes.len() != expected {
            return Err("native frame plane count does not match chroma layout".into());
        }
        let mut planes = Vec::with_capacity(3);
        for plane in &frame.planes {
            let bytes = usize::from(plane.bytes_per_sample);
            if !matches!(bytes, 1 | 2)
                || plane.width == 0
                || plane.height == 0
                || plane
                    .width
                    .checked_mul(bytes)
                    .is_none_or(|row| plane.stride < row)
                || plane
                    .stride
                    .checked_mul(plane.height)
                    .is_none_or(|size| size > plane.data.len())
            {
                return Err("native frame has invalid plane storage".into());
            }
            let format = if bytes == 1 {
                wgpu::TextureFormat::R8Uint
            } else {
                wgpu::TextureFormat::R16Uint
            };
            let texture = texture(
                &self.device,
                plane.width as u32,
                plane.height as u32,
                format,
                "native-yuv-plane",
            );
            self.queue.write_texture(
                texture.as_image_copy(),
                &plane.data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(plane.stride as u32),
                    rows_per_image: Some(plane.height as u32),
                },
                texture.size(),
            );
            planes.push(texture);
        }
        while planes.len() < 3 {
            planes.push(planes[0].clone());
        }
        let chroma_divisor = match frame.chroma {
            ChromaSampling::Yuv420 => [2, 2],
            ChromaSampling::Yuv422 => [2, 1],
            _ => [1, 1],
        };
        Ok(YuvInput {
            planes: planes.try_into().map_err(|_| "invalid plane count")?,
            descriptor: FrameDescriptor {
                width: frame.width,
                height: frame.height,
                bit_depth: frame.bit_depth,
                crop: frame.crop,
                rotation_degrees: frame.rotation_degrees,
                color: frame.color.clone(),
            },
            chroma_divisor,
            interleaved: false,
            high_shift: 0,
            monochrome: frame.chroma == ChromaSampling::Monochrome,
        })
    }

    pub fn convert(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        input: YuvInput,
        target: DisplayTarget,
    ) -> Result<ConvertedFrame, String> {
        self.prepare_color(&input.descriptor, target)?;
        let color = self.color.as_ref().expect("prepared color resources");
        let crop = input.descriptor.crop;
        let coded_width = input.descriptor.width;
        let coded_height = input.descriptor.height;
        if crop.width == 0
            || crop.height == 0
            || crop
                .x
                .checked_add(crop.width)
                .is_none_or(|end| end > coded_width)
            || crop
                .y
                .checked_add(crop.height)
                .is_none_or(|end| end > coded_height)
        {
            return Err("invalid native frame crop".into());
        }
        let rotation = input.descriptor.rotation_degrees;
        if !matches!(rotation, 0 | 90 | 180 | 270) {
            return Err("unsupported frame rotation".into());
        }
        let (width, height) = if matches!(rotation, 90 | 270) {
            (crop.height, crop.width)
        } else {
            (crop.width, crop.height)
        };
        if width > self.device.limits().max_texture_dimension_2d as usize
            || height > self.device.limits().max_texture_dimension_2d as usize
        {
            return Err("native frame exceeds the GPU texture limit".into());
        }
        let output = texture(
            &self.device,
            width as u32,
            height as u32,
            wgpu::TextureFormat::Rgba16Float,
            "native-video-linear",
        );
        let offsets = chroma_offsets(input.descriptor.color.chroma_location, input.chroma_divisor);
        let values = [
            crop.x as f32,
            crop.y as f32,
            crop.width as f32,
            crop.height as f32,
            input.chroma_divisor[0] as f32,
            input.chroma_divisor[1] as f32,
            offsets[0],
            offsets[1],
            f32::from(rotation),
            if input.interleaved { 1.0 } else { 0.0 },
            input.high_shift as f32,
            if input.monochrome { 1.0 } else { 0.0 },
        ];
        let geometry = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("native-yuv-geometry"),
            size: GEOMETRY_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bytes = values
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        self.queue.write_buffer(&geometry, 0, &bytes);
        let views = input
            .planes
            .each_ref()
            .map(|plane| plane.create_view(&Default::default()));
        let lut = color.lut.create_view(&Default::default());
        let destination = output.create_view(&Default::default());
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native-yuv-color"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&destination),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: color.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&lut),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: geometry.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(
            (width as u32).div_ceil(WORKGROUP),
            (height as u32).div_ceil(WORKGROUP),
            1,
        );
        drop(pass);
        let mut descriptor = input.descriptor.clone();
        descriptor.width = width;
        descriptor.height = height;
        descriptor.crop = hongguo_media_types::CropRect {
            x: 0,
            y: 0,
            width,
            height,
        };
        descriptor.rotation_degrees = 0;
        if matches!(rotation, 90 | 270) {
            descriptor.color.sample_aspect_ratio = descriptor
                .color
                .sample_aspect_ratio
                .map(|(num, den)| (den, num));
        }
        Ok(ConvertedFrame {
            texture: output,
            descriptor,
            _input: input,
        })
    }

    fn prepare_color(
        &mut self,
        descriptor: &FrameDescriptor,
        target: DisplayTarget,
    ) -> Result<(), String> {
        let key = (
            descriptor.color.clone(),
            descriptor.bit_depth,
            descriptor.height,
            target,
        );
        if self.color.as_ref().is_some_and(|color| color.key == key) {
            return Ok(());
        }
        let transform = ColorTransform::new(
            &descriptor.color,
            descriptor.bit_depth,
            descriptor.height,
            target,
        )?;
        let bytes = transform.params.bytes();
        let params = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("native-video-color"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&params, 0, &bytes);
        let lut = texture(
            &self.device,
            LUT_WIDTH,
            LUT_HEIGHT,
            wgpu::TextureFormat::Rgba32Float,
            "native-video-color-lut",
        );
        self.queue.write_texture(
            lut.as_image_copy(),
            &transform.lut_bytes(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(LUT_WIDTH * 16),
                rows_per_image: Some(LUT_HEIGHT),
            },
            lut.size(),
        );
        self.color = Some(ColorResources { key, params, lut });
        Ok(())
    }

    pub fn map_output(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Texture,
        descriptor: &FrameDescriptor,
        target: DisplayTarget,
    ) -> Result<wgpu::Texture, String> {
        self.prepare_color(descriptor, target)?;
        let color = self.color.as_ref().expect("prepared color resources");
        let output = texture(
            &self.device,
            input.width(),
            input.height(),
            wgpu::TextureFormat::Rgba16Float,
            "native-video-output",
        );
        let source = input.create_view(&Default::default());
        let destination = output.create_view(&Default::default());
        let lut = color.lut.create_view(&Default::default());
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native-color-map"),
            layout: &self.mapping.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&destination),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: color.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&lut),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.mapping);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(
            input.width().div_ceil(WORKGROUP),
            input.height().div_ceil(WORKGROUP),
            1,
        );
        drop(pass);
        Ok(output)
    }
}

fn texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    label: &str,
) -> wgpu::Texture {
    let mut usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
    if format == wgpu::TextureFormat::Rgba16Float {
        usage |= wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC;
    }
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn chroma_offsets(location: Option<u8>, divisor: [u32; 2]) -> [f32; 2] {
    let centered = [(divisor[0] - 1) as f32 * 0.5, (divisor[1] - 1) as f32 * 0.5];
    match location {
        Some(1) => [0.0, centered[1]],
        Some(3) => [0.0, 0.0],
        Some(4) => [centered[0], 0.0],
        Some(5) => [0.0, (divisor[1] - 1) as f32],
        Some(6) => [centered[0], (divisor[1] - 1) as f32],
        _ => centered,
    }
}

#[cfg(test)]
mod tests;

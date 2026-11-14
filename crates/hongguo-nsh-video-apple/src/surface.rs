use crate::OutputError;
use hongguo_nsh_video_api::{ErrorCode, ResizeMode};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_metal::{
    MTLCommandBuffer, MTLCommandQueue, MTLDrawable, MTLPixelFormat, MTLTexture, MTLTextureType,
};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer, CATransaction};
use std::{borrow::Cow, sync::Arc};

pub(crate) struct SurfaceTarget {
    texture: wgpu::Texture,
    drawable: Retained<ProtocolObject<dyn MTLDrawable>>,
}

pub struct MetalSurface {
    layer: Retained<CAMetalLayer>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    width: u32,
    height: u32,
    drawable: bool,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    fit: wgpu::Buffer,
}

impl MetalSurface {
    pub async fn new(layer: Retained<CAMetalLayer>) -> Result<Self, OutputError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                ..Default::default()
            })
            .await
            .map_err(gpu_error)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: device_limits(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(gpu_error)?;
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        {
            let metal = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }
                .ok_or_else(|| gpu_error("Metal adapter unavailable"))?;
            layer.setDevice(Some(metal.raw_device()));
        }
        layer.setPixelFormat(MTLPixelFormat::RGBA16Float);
        layer.setFramebufferOnly(true);
        layer.setOpaque(true);
        layer.setPresentsWithTransaction(false);
        layer.setMaximumDrawableCount(2);
        layer.setAllowsNextDrawableTimeout(true);
        let color_space = objc2_core_graphics::CGColorSpace::with_name(Some(unsafe {
            objc2_core_graphics::kCGColorSpaceExtendedLinearSRGB
        }));
        layer.setColorspace(color_space.as_deref());
        layer.setWantsExtendedDynamicRangeContent(true);
        CATransaction::commit();
        CATransaction::flush();
        let errors = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video-present"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("present.wgsl"))),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video-present"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let fit = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video-present-fit"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if let Some(error) = errors.pop().await {
            return Err(gpu_error(error));
        }
        Ok(Self {
            layer,
            device: Arc::new(device),
            queue: Arc::new(queue),
            width: 0,
            height: 0,
            drawable: false,
            pipeline,
            sampler,
            fit,
        })
    }

    pub fn device(&self) -> Arc<wgpu::Device> {
        self.device.clone()
    }
    pub fn queue(&self) -> Arc<wgpu::Queue> {
        self.queue.clone()
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.drawable = width > 0 && height > 0;
        if self.drawable {
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            self.layer
                .setDrawableSize(objc2_core_foundation::CGSize::new(
                    f64::from(width),
                    f64::from(height),
                ));
            CATransaction::commit();
            CATransaction::flush();
        }
    }

    pub(crate) fn acquire(&self) -> Result<Option<SurfaceTarget>, OutputError> {
        if !self.drawable {
            return Ok(None);
        }
        let Some(drawable) = self.layer.nextDrawable() else {
            return Ok(None);
        };
        let raw = drawable.texture();
        if raw.width() != self.width as usize || raw.height() != self.height as usize {
            return Ok(None);
        }
        if self.width > self.device.limits().max_texture_dimension_2d
            || self.height > self.device.limits().max_texture_dimension_2d
        {
            return Err(OutputError::new(
                ErrorCode::InputTooLarge,
                "native video target exceeds GPU dimensions",
            ));
        }
        let size = wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        };
        let native = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                raw,
                wgpu::TextureFormat::Rgba16Float,
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
            self.device
                .create_texture_from_hal::<wgpu::hal::api::Metal>(
                    native,
                    &wgpu::TextureDescriptor {
                        label: Some("native-metal-drawable"),
                        size,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba16Float,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    },
                )
        };
        Ok(Some(SurfaceTarget {
            texture,
            drawable: ProtocolObject::from_retained(drawable),
        }))
    }

    pub(crate) fn encode_present(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &SurfaceTarget,
        image: &wgpu::TextureView,
        source_aspect: f32,
        mode: ResizeMode,
    ) -> Result<(), OutputError> {
        let transform = fit_aspect(source_aspect, self.width, self.height, mode)?;
        let bytes = transform
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        self.queue.write_buffer(&self.fit, 0, &bytes);
        let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("video-present"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(image),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.fit.as_entire_binding(),
                },
            ],
        });
        let view = target.texture.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("video-present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }

    pub(crate) fn present(
        &self,
        target: SurfaceTarget,
        presented: impl Fn(f64) + Send + Sync + 'static,
        completed: impl Fn() + Send + Sync + 'static,
    ) -> Result<(), OutputError> {
        let block = block2::RcBlock::new(
            move |drawable: std::ptr::NonNull<ProtocolObject<dyn MTLDrawable>>| {
                presented(unsafe { drawable.as_ref() }.presentedTime());
            },
        );
        unsafe {
            target
                .drawable
                .addPresentedHandler(&*block as *const _ as *mut _);
        }
        let metal = unsafe { self.queue.as_hal::<wgpu::hal::api::Metal>() }
            .ok_or_else(|| gpu_error("Metal queue unavailable"))?;
        let command = metal
            .as_raw()
            .commandBuffer()
            .ok_or_else(|| gpu_error("cannot create native presentation command"))?;
        let done = block2::RcBlock::new(
            move |_: std::ptr::NonNull<ProtocolObject<dyn MTLCommandBuffer>>| completed(),
        );
        // Native completion runs independently of WGPU polling, including after
        // the widget/worker retires. It follows all work on this same queue.
        unsafe {
            command.addCompletedHandler(&*done as *const _ as *mut _);
        }
        command.presentDrawable(&target.drawable);
        command.commit();
        Ok(())
    }
}

fn gpu_error(error: impl std::fmt::Display) -> OutputError {
    OutputError::new(ErrorCode::GpuFailed, error.to_string())
}

fn device_limits(supported: wgpu::Limits) -> wgpu::Limits {
    // iOS Metal can expose fewer shader variables than WebGPU defaults; keep
    // conservative requirements while retaining portrait texture capacity.
    wgpu::Limits::downlevel_defaults().using_resolution(supported)
}

#[cfg(test)]
fn fit_transform(
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
    mode: ResizeMode,
) -> Result<[f32; 4], OutputError> {
    fit_aspect(
        source_width as f32 / source_height as f32,
        width,
        height,
        mode,
    )
}

fn fit_aspect(
    source: f32,
    width: u32,
    height: u32,
    mode: ResizeMode,
) -> Result<[f32; 4], OutputError> {
    if !source.is_finite() || source <= 0.0 || width == 0 || height == 0 {
        return Err(OutputError::new(
            ErrorCode::FrameFormatUnsupported,
            "cannot fit empty video dimensions",
        ));
    }
    let target = width as f32 / height as f32;
    let (x, y) = match mode {
        ResizeMode::Contain if source > target => (1.0, source / target),
        ResizeMode::Contain => (target / source, 1.0),
        ResizeMode::Cover if source > target => (target / source, 1.0),
        ResizeMode::Cover => (1.0, source / target),
        ResizeMode::Stretch => (1.0, 1.0),
    };
    Ok([x, y, (1.0 - x) * 0.5, (1.0 - y) * 0.5])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_device_limits_fit_the_adapter_and_preserve_portrait_upscaling() {
        let supported =
            wgpu::Limits::downlevel_defaults().using_resolution(wgpu::Limits::default());
        let required = device_limits(supported.clone());
        assert!(
            required.check_limits(&supported),
            "device creation must respect the adapter limits"
        );
        assert!(
            required.max_texture_dimension_2d >= 3_840,
            "1080p portrait upscaling must retain the adapter texture capacity"
        );
        assert_eq!(
            required.max_immediate_size, 0,
            "the renderer does not request the IMMEDIATES feature"
        );
    }

    #[test]
    fn contain_leaves_centered_bars_and_cover_crops_the_same_center() {
        let contain = fit_transform(1920, 1080, 100, 100, ResizeMode::Contain).unwrap();
        let cover = fit_transform(1920, 1080, 100, 100, ResizeMode::Cover).unwrap();
        for (actual, expected) in contain.into_iter().zip([1.0, 16.0 / 9.0, 0.0, -7.0 / 18.0]) {
            assert!((actual - expected).abs() < 0.000_01);
        }
        assert_eq!(cover, [0.5625, 1.0, 0.21875, 0.0]);
        assert_eq!(
            fit_transform(1920, 1080, 100, 100, ResizeMode::Stretch).unwrap(),
            [1.0, 1.0, 0.0, 0.0]
        );
    }

    #[test]
    fn empty_source_or_target_never_produces_invalid_texture_coordinates() {
        for (source_width, source_height, width, height) in
            [(0, 1, 1, 1), (1, 0, 1, 1), (1, 1, 0, 1), (1, 1, 1, 0)]
        {
            assert_eq!(
                fit_transform(
                    source_width,
                    source_height,
                    width,
                    height,
                    ResizeMode::Contain
                )
                .unwrap_err()
                .code(),
                ErrorCode::FrameFormatUnsupported
            );
        }
    }
}

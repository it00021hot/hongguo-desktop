use crate::OutputError;
use hongguo_nsh_video_api::{ErrorCode, ResizeMode};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_metal::{
    MTLCommandBuffer, MTLCommandQueue, MTLDevice, MTLDrawable, MTLPixelFormat, MTLTexture,
    MTLTextureType,
};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer, CATransaction};
use std::sync::Arc;

pub(crate) const MAX_DRAWABLES: usize = 2;

pub(crate) struct SurfaceTarget {
    texture: wgpu::Texture,
    drawable: Retained<ProtocolObject<dyn MTLDrawable>>,
}

pub struct MetalSurface {
    layer: Retained<CAMetalLayer>,
    gpu: Arc<crate::gpu::Gpu>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    width: u32,
    height: u32,
    drawable: bool,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    fit: wgpu::Buffer,
    fit_transform: Option<[f32; 4]>,
}

impl MetalSurface {
    pub async fn new(layer: Retained<CAMetalLayer>) -> Result<Self, OutputError> {
        let gpu = crate::gpu::Gpu::shared()?;
        let device = gpu.device.clone();
        let queue = gpu.queue.clone();
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
        layer.setMaximumDrawableCount(MAX_DRAWABLES);
        layer.setAllowsNextDrawableTimeout(true);
        let color_space = objc2_core_graphics::CGColorSpace::with_name(Some(unsafe {
            objc2_core_graphics::kCGColorSpaceExtendedLinearSRGB
        }));
        layer.setColorspace(color_space.as_deref());
        layer.setWantsExtendedDynamicRangeContent(true);
        CATransaction::commit();
        CATransaction::flush();
        let pipeline = gpu.presentation.clone();
        let sampler = gpu.sampler.clone();
        let fit = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video-present-fit"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self {
            layer,
            gpu,
            device,
            queue,
            width: 0,
            height: 0,
            drawable: false,
            pipeline,
            sampler,
            fit,
            fit_transform: None,
        })
    }

    pub fn device(&self) -> Arc<wgpu::Device> {
        self.device.clone()
    }

    pub(crate) fn allocated_bytes(&self) -> Option<usize> {
        unsafe { self.device.as_hal::<wgpu::hal::api::Metal>() }
            .map(|metal| metal.raw_device().currentAllocatedSize())
    }
    pub fn queue(&self) -> Arc<wgpu::Queue> {
        self.queue.clone()
    }

    pub(crate) fn converter(&self) -> hongguo_video_gpu::YuvConverter {
        hongguo_video_gpu::YuvConverter::with_pipelines(
            self.device(),
            self.queue(),
            self.gpu.color.clone(),
        )
    }

    pub(crate) fn check(&self) -> Result<(), OutputError> {
        self.gpu.check()
    }

    pub(crate) fn invalidate(&self, detail: String) {
        self.gpu.invalidate(detail);
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
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &SurfaceTarget,
        image: &wgpu::TextureView,
        source_aspect: f32,
        mode: ResizeMode,
    ) -> Result<(), OutputError> {
        let transform = fit_aspect(source_aspect, self.width, self.height, mode)?;
        if self.fit_transform != Some(transform) {
            let bytes = transform
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>();
            self.queue.write_buffer(&self.fit, 0, &bytes);
            self.fit_transform = Some(transform);
        }
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
        display_deadline: f64,
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
        command.presentDrawable_atTime(&target.drawable, display_deadline);
        command.commit();
        Ok(())
    }
}

fn gpu_error(error: impl std::fmt::Display) -> OutputError {
    OutputError::new(ErrorCode::GpuFailed, error.to_string())
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

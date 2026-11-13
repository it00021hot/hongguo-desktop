use crate::OutputError;
use hongguo_nsh_video_api::{ErrorCode, ResizeMode};
use objc2::rc::Retained;
use objc2_quartz_core::CAMetalLayer;
use std::{borrow::Cow, sync::Arc};

pub struct MetalSurface {
    surface: wgpu::Surface<'static>,
    _layer: Retained<CAMetalLayer>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    config: wgpu::SurfaceConfiguration,
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
        // SAFETY: This surface retains the same layer for its complete lifetime.
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(
                Retained::as_ptr(&layer).cast_mut().cast(),
            ))
        }
        .map_err(gpu_error)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(gpu_error)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(gpu_error)?;
        let mut config = surface.get_default_config(&adapter, 1, 1).ok_or_else(|| {
            OutputError::new(
                ErrorCode::GpuUnavailable,
                "Metal surface has no supported configuration",
            )
        })?;
        if !surface
            .get_capabilities(&adapter)
            .formats
            .contains(&wgpu::TextureFormat::Bgra8Unorm)
        {
            return Err(OutputError::new(
                ErrorCode::GpuUnavailable,
                "Metal surface cannot present BGRA8",
            ));
        }
        config.format = wgpu::TextureFormat::Bgra8Unorm;
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        config.desired_maximum_frame_latency = 1;
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
                    format: config.format,
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
            surface,
            _layer: layer,
            device: Arc::new(device),
            queue: Arc::new(queue),
            config,
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
        self.drawable = width > 0 && height > 0;
        if !self.drawable {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.configure();
    }

    fn configure(&self) {
        self.surface.configure(&self.device, &self.config);
        // WGPU's Metal backend turns this off. A public bounded acquisition is
        // required when UIKit hides/detaches the layer during an in-flight draw.
        self._layer.setAllowsNextDrawableTimeout(true);
    }

    pub fn present(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        image: &wgpu::TextureView,
        source_width: u32,
        source_height: u32,
        mode: ResizeMode,
    ) -> Result<Option<wgpu::SurfaceTexture>, OutputError> {
        let Some(target) = self.acquire()? else {
            return Ok(None);
        };
        self.encode_present(encoder, &target, image, source_width, source_height, mode)?;
        Ok(Some(target))
    }

    pub(crate) fn acquire(&mut self) -> Result<Option<wgpu::SurfaceTexture>, OutputError> {
        if !self.drawable {
            return Ok(None);
        }
        let target = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(target)
            | wgpu::CurrentSurfaceTexture::Suboptimal(target) => target,
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(None);
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.configure();
                return Ok(None);
            }
            status => {
                return Err(OutputError::new(
                    ErrorCode::GpuFailed,
                    format!("Metal surface acquisition: {status:?}"),
                ));
            }
        };
        Ok(Some(target))
    }

    pub(crate) fn encode_present(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::SurfaceTexture,
        image: &wgpu::TextureView,
        source_width: u32,
        source_height: u32,
        mode: ResizeMode,
    ) -> Result<(), OutputError> {
        let transform = fit_transform(
            source_width,
            source_height,
            self.config.width,
            self.config.height,
            mode,
        )?;
        let mut bytes = [0_u8; 16];
        for (destination, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(transform) {
            destination.copy_from_slice(&value.to_le_bytes());
        }
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
        drop(pass);
        Ok(())
    }
}

fn gpu_error(error: impl std::fmt::Display) -> OutputError {
    OutputError::new(ErrorCode::GpuFailed, error.to_string())
}

fn fit_transform(
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
    mode: ResizeMode,
) -> Result<[f32; 4], OutputError> {
    if source_width == 0 || source_height == 0 || width == 0 || height == 0 {
        return Err(OutputError::new(
            ErrorCode::FrameFormatUnsupported,
            "cannot fit empty video dimensions",
        ));
    }
    let source = source_width as f32 / source_height as f32;
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

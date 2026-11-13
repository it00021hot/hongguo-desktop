use crate::bridge::error;
use hongguo_nsh_video_api::ResizeMode;
use ndk::native_window::NativeWindow;
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use std::{borrow::Cow, sync::Arc};

pub(crate) struct OutputSurface {
    surface: wgpu::Surface<'static>,
    _window: NativeWindow,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    fit: wgpu::Buffer,
}
impl OutputSurface {
    pub async fn new(window: NativeWindow, width: u32, height: u32) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::GL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        // SAFETY: ANativeWindow was acquired from the exact TextureView Surface;
        // this owner retains it until after the WGPU surface has been destroyed.
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(RawDisplayHandle::Android(AndroidDisplayHandle::new())),
                raw_window_handle: RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(
                    window.ptr().cast(),
                )),
            })
        }
        .map_err(error)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(error)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(error)?;
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or_else(|| "GLES surface has no supported configuration".to_owned())?;
        config.desired_maximum_frame_latency = 1;
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("android-video-present"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("present.wgsl"))),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("android-video-present"),
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
            label: Some("android-video-fit"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if let Some(error) = scope.pop().await {
            return Err(error.to_string());
        }
        window
            .set_buffers_geometry(
                config.width as i32,
                config.height as i32,
                Some(window.format()),
            )
            .map_err(error)?;
        surface.configure(&device, &config);
        Ok(Self {
            surface,
            _window: window,
            device: Arc::new(device),
            queue: Arc::new(queue),
            config,
            pipeline,
            sampler,
            fit,
        })
    }
    pub fn resize(&mut self, width: u32, height: u32) -> Result<bool, String> {
        if width == 0 || height == 0 || (width, height) == (self.config.width, self.config.height) {
            return Ok(false);
        }
        self.config.width = width;
        self.config.height = height;
        self._window
            .set_buffers_geometry(width as i32, height as i32, Some(self._window.format()))
            .map_err(error)?;
        self.surface.configure(&self.device, &self.config);
        Ok(true)
    }
    pub fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Texture,
        mode: ResizeMode,
    ) -> Result<Option<wgpu::SurfaceTexture>, String> {
        let target = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(target)
            | wgpu::CurrentSurfaceTexture::Suboptimal(target) => target,
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(None);
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(None);
            }
            status => return Err(format!("GLES presentation unavailable: {status:?}")),
        };
        let source = input.width() as f32 / input.height() as f32;
        let target_ratio = self.config.width as f32 / self.config.height as f32;
        let (x, y) = match mode {
            ResizeMode::Contain if source > target_ratio => (1.0, source / target_ratio),
            ResizeMode::Contain => (target_ratio / source, 1.0),
            ResizeMode::Cover if source > target_ratio => (target_ratio / source, 1.0),
            ResizeMode::Cover => (1.0, source / target_ratio),
            ResizeMode::Stretch => (1.0, 1.0),
        };
        let bytes = [x, y, (1.0 - x) * 0.5, (1.0 - y) * 0.5]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        self.queue.write_buffer(&self.fit, 0, &bytes);
        let input = input.create_view(&Default::default());
        let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("android-video-present"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input),
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
            label: Some("android-video-present"),
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
        Ok(Some(target))
    }
}

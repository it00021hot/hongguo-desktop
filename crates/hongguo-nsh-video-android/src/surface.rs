use crate::bridge::error;
use hongguo_nsh_video_api::ResizeMode;
use hongguo_video_color::DisplayTarget;
use ndk::native_window::NativeWindow;
use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

pub(crate) struct OutputSurface {
    surface: wgpu::Surface<'static>,
    _window: NativeWindow,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    fit: wgpu::Buffer,
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    floating: bool,
    hdr_peak: f32,
}
impl OutputSurface {
    pub fn set_presentation_time(&self, token: i64) -> Result<(), String> {
        // SAFETY: This surface belongs to the retained GLES device. HAL consumes
        // the token at its actual EGL swap, never at an earlier queue submission.
        let surface = unsafe { self.surface.as_hal::<wgpu::hal::gles::Api>() }
            .ok_or("GLES surface required")?;
        surface.set_next_presentation_time(token).map_err(error)
    }
    pub fn swap_succeeded(&self, token: i64) -> bool {
        // SAFETY: The surface is owned by this GLES renderer.
        unsafe { self.surface.as_hal::<wgpu::hal::gles::Api>() }
            .is_some_and(|surface| token > 0 && surface.submitted_presentation_time() == token)
    }
    pub fn target(&self) -> DisplayTarget {
        crate::output_color::target(self.floating, self.hdr_peak)
    }
    fn verify_window(
        surface: &wgpu::Surface<'static>,
        window: &NativeWindow,
        floating: bool,
        hdr: bool,
    ) -> Result<(), String> {
        // SAFETY: This surface was created by the retained GLES instance.
        let configured = !floating
            || unsafe { surface.as_hal::<wgpu::hal::gles::Api>() }
                .is_some_and(|surface| surface.android_linear_output_configured(hdr));
        if !configured
            || (floating
                && window.format()
                    != ndk::hardware_buffer_format::HardwareBufferFormat::R16G16B16A16_FLOAT)
        {
            return Err("configured output does not match its RGBA16F linear colorspace".into());
        }
        let dataspace = if hdr {
            ndk::data_space::DataSpace::ScrgbLinear
        } else if floating {
            ndk::data_space::DataSpace::SrgbLinear
        } else {
            ndk::data_space::DataSpace::Srgb
        };
        window.set_buffers_data_space(dataspace).map_err(error)?;
        if window.buffers_data_space().map_err(error)? != dataspace {
            return Err("native window did not retain the requested SDR/HDR dataspace".into());
        }
        Ok(())
    }
    pub fn rebind(
        &mut self,
        window: NativeWindow,
        width: u32,
        height: u32,
        hdr_peak: f32,
    ) -> Result<(), String> {
        if hdr_peak > 0. && !self.floating {
            return Err("HDR output requires a verified floating-point renderer".into());
        }
        let surface = Self::create_surface(&self.instance, &window, self.floating, hdr_peak > 0.)?;
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        if !surface
            .get_capabilities(&self.adapter)
            .formats
            .contains(&self.config.format)
        {
            return Err(
                "replacement output does not support the existing framebuffer format".into(),
            );
        }
        surface.configure(&self.device, &self.config);
        Self::verify_window(&surface, &window, self.floating, hdr_peak > 0.)?;
        self.surface = surface;
        self._window = window;
        self.hdr_peak = hdr_peak;
        Ok(())
    }
    fn create_surface(
        instance: &wgpu::Instance,
        window: &NativeWindow,
        floating: bool,
        hdr: bool,
    ) -> Result<wgpu::Surface<'static>, String> {
        // SAFETY: The caller retains this acquired ANativeWindow beyond the surface.
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(RawDisplayHandle::Android(AndroidDisplayHandle::new())),
                raw_window_handle: RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(
                    window.ptr().cast(),
                )),
            })
        }
        .map_err(error)?;
        if floating {
            // SAFETY: This instance exclusively uses GLES, with a verified float config.
            unsafe { surface.as_hal::<wgpu::hal::gles::Api>() }
                .ok_or("GLES surface required")?
                .set_android_linear_output(hdr)
                .map_err(error)?;
        }
        Ok(surface)
    }
    pub async fn new(
        window: NativeWindow,
        width: u32,
        height: u32,
        hdr_peak: f32,
        float_output: bool,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::GL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let floating = float_output || hdr_peak > 0.;
        if floating {
            // SAFETY: No surface or adapter has been obtained from this new instance.
            unsafe {
                instance
                    .as_hal::<wgpu::hal::gles::Api>()
                    .ok_or("GLES instance required")?
                    .enable_android_linear_scrgb()
            }
            .map_err(error)?;
        }
        let surface = Self::create_surface(&instance, &window, floating, hdr_peak > 0.)?;
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
        let failures = Arc::new(Mutex::new(None));
        let rejected = failures.clone();
        device.on_uncaptured_error(Arc::new(move |error| {
            *rejected.lock().expect("Android surface setup") = Some(error.to_string());
        }));
        let mut config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or_else(|| "GLES surface has no supported configuration".to_owned())?;
        config.desired_maximum_frame_latency = 1;
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        // Shared map_output already encodes SDR sRGB. An sRGB attachment would
        // encode it a second time and alter both luma and color.
        config.format = surface
            .get_capabilities(&adapter)
            .formats
            .into_iter()
            .find(|format| {
                if floating {
                    *format == wgpu::TextureFormat::Rgba16Float
                } else {
                    !format.is_srgb() && *format != wgpu::TextureFormat::Rgba16Float
                }
            })
            .ok_or("GLES surface has no linear UNORM format")?;
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
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        window
            .set_buffers_geometry(
                config.width as i32,
                config.height as i32,
                Some(window.format()),
            )
            .map_err(error)?;
        surface.configure(&device, &config);
        if let Some(error) = scope.pop().await {
            return Err(error.to_string());
        }
        if let Some(error) = failures.lock().expect("Android surface setup").take() {
            return Err(error);
        }
        Self::verify_window(&surface, &window, floating, hdr_peak > 0.)?;
        Ok(Self {
            surface,
            _window: window,
            device: Arc::new(device),
            queue: Arc::new(queue),
            config,
            pipeline,
            sampler,
            fit,
            instance,
            adapter,
            floating,
            hdr_peak,
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
        source_ratio: Option<f32>,
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
        let source = source_ratio.unwrap_or(input.width() as f32 / input.height() as f32);
        let target_ratio = self.config.width as f32 / self.config.height as f32;
        let (x, y) = match mode {
            ResizeMode::Contain if source > target_ratio => (1.0, source / target_ratio),
            ResizeMode::Contain => (target_ratio / source, 1.0),
            ResizeMode::Cover if source > target_ratio => (target_ratio / source, 1.0),
            ResizeMode::Cover => (1.0, source / target_ratio),
            ResizeMode::Stretch => (1.0, 1.0),
        };
        let bytes = [
            x,
            y,
            (1.0 - x) * 0.5,
            (1.0 - y) * 0.5,
            if self.floating { 1. } else { 0. },
            0.,
            0.,
            0.,
        ]
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

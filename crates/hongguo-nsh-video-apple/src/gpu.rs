use crate::OutputError;
use hongguo_nsh_video_api::ErrorCode;
use std::{
    borrow::Cow,
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::Instant,
};

pub(crate) struct Gpu {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub presentation: wgpu::RenderPipeline,
    pub sampler: wgpu::Sampler,
    pub color: hongguo_video_gpu::YuvPipelines,
    failure: Arc<Mutex<Option<String>>>,
}

#[derive(Default)]
struct Cache {
    gpu: Option<Arc<Gpu>>,
    initializing: bool,
}

impl Gpu {
    pub fn shared() -> Result<Arc<Self>, OutputError> {
        static CACHE: OnceLock<(Mutex<Cache>, Condvar)> = OnceLock::new();
        let (cache, changed) = CACHE.get_or_init(Default::default);
        let mut state = cache.lock().expect("video GPU cache");
        loop {
            if let Some(gpu) = state.gpu.as_ref().filter(|gpu| gpu.check().is_ok()) {
                return Ok(gpu.clone());
            }
            if !state.initializing {
                state.initializing = true;
                break;
            }
            state = changed.wait(state).expect("video GPU initialization");
        }
        drop(state);
        let result = pollster::block_on(Self::create()).map(Arc::new);
        let mut state = cache.lock().expect("video GPU cache");
        state.initializing = false;
        state.gpu = result.as_ref().ok().cloned();
        changed.notify_all();
        result
    }

    pub fn check(&self) -> Result<(), OutputError> {
        match self.failure.lock().expect("video GPU failure").as_ref() {
            Some(detail) => Err(gpu_error(detail)),
            None => Ok(()),
        }
    }

    pub fn invalidate(&self, detail: String) {
        *self.failure.lock().expect("video GPU failure") = Some(detail);
        self.device.destroy();
    }

    async fn create() -> Result<Self, OutputError> {
        let started = Instant::now();
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
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(gpu_error)?;
        let failure = Arc::new(Mutex::new(None));
        let lost = failure.clone();
        device.set_device_lost_callback(move |reason, detail| {
            *lost.lock().expect("video GPU failure") = Some(format!("{reason:?}: {detail}"));
        });
        let uncaptured = failure.clone();
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            *uncaptured.lock().expect("video GPU failure") = Some(error.to_string());
        }));
        let errors = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video-present"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("present.wgsl"))),
        });
        let presentation = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
        let color = hongguo_video_gpu::YuvPipelines::new(&device);
        if let Some(error) = errors.pop().await {
            return Err(gpu_error(error));
        }
        log::debug!(target: "hongguo_nsh_video_apple::gpu", "created shared video GPU initializationMicros={}", started.elapsed().as_micros());
        Ok(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            presentation,
            sampler,
            color,
            failure,
        })
    }
}

fn gpu_error(error: impl std::fmt::Display) -> OutputError {
    OutputError::new(ErrorCode::GpuFailed, error.to_string())
}

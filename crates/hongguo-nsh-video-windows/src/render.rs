//! Background-only GPU pipeline and double-buffered D3D11/D2D publication.
use crate::{
    DisplayedFrame, FrameLease, SharedFrames, VideoError,
    output::{OutputLease, OutputSlot},
};
use hongguo_enhancement::Pipeline;
use hongguo_nsh_video_api::{Enhancement, ErrorCode, FrameMetadata};
use hongguo_video_color::{ColorTransform, LUT_HEIGHT, LUT_WIDTH};
use std::{
    borrow::Cow,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::Graphics::{
        Direct3D11::{ID3D11Device, ID3D11Texture2D},
        Dxgi::IDXGIDevice,
    },
    core::Interface,
};

pub struct CompletedFrame {
    epoch: u64,
    elapsed: Duration,
    frame: Arc<DisplayedFrame>,
}
impl CompletedFrame {
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }
    pub fn frame(&self) -> &Arc<DisplayedFrame> {
        &self.frame
    }
}

pub struct GpuRenderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    decoder: ID3D11Device,
    input: SharedFrames,
    pipeline: Pipeline,
    blit: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    outputs: Vec<Arc<OutputSlot>>,
    output_size: (u32, u32),
    busy: Arc<AtomicBool>,
    completed: Arc<Mutex<Option<CompletedFrame>>>,
    failure: Arc<Mutex<Option<String>>>,
    color: Option<FinalColor>,
    output_format: wgpu::TextureFormat,
}

struct FinalColor {
    transform: Arc<ColorTransform>,
    uniform: wgpu::Buffer,
    lut: wgpu::Texture,
}

impl GpuRenderer {
    pub async fn new(
        decoder: &ID3D11Device,
        width: u32,
        height: u32,
        mode: nsh_platform_windows::OutputColorMode,
    ) -> Result<Self, VideoError> {
        let output_format = if mode == nsh_platform_windows::OutputColorMode::ScRgb {
            wgpu::TextureFormat::Rgba16Float
        } else {
            wgpu::TextureFormat::Bgra8Unorm
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        // SAFETY: This decoder is retained and used only through public DXGI identity queries.
        let expected = unsafe {
            decoder
                .cast::<IDXGIDevice>()
                .map_err(gpu)?
                .GetAdapter()
                .map_err(gpu)?
                .GetDesc()
                .map_err(gpu)?
        }
        .AdapterLuid;
        let mut matching = None;
        for adapter in instance.enumerate_adapters(wgpu::Backends::DX12).await {
            // SAFETY: The guard exposes this live adapter's read-only DXGI identity.
            let matches =
                unsafe { adapter.as_hal::<wgpu::hal::api::Dx12>() }.is_some_and(|native| {
                    // SAFETY: Public metadata query on the retained native DXGI adapter.
                    unsafe { native.raw_adapter().GetDesc1() }.is_ok_and(|description| {
                        description.AdapterLuid.LowPart == expected.LowPart
                            && description.AdapterLuid.HighPart == expected.HighPart
                    })
                });
            if matches {
                matching = Some(adapter);
                break;
            }
        }
        let adapter =
            matching.ok_or_else(|| gpu("no DX12 adapter matches the native video device"))?;
        let (device, queue) = adapter
            .request_device(&Default::default())
            .await
            .map_err(gpu)?;
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let failure = Arc::new(Mutex::new(None));
        let lost = failure.clone();
        device.set_device_lost_callback(move |reason, detail| {
            *lost
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(format!("{reason:?}: {detail}"));
        });
        let errors = failure.clone();
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            *errors
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
        }));
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video-to-shared-scrgb"),
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(format!(
                "{}\n{}",
                hongguo_video_color::WGSL,
                include_str!("blit.wgsl")
            ))),
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video-to-shared-scrgb"),
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
                    format: output_format,
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
        if let Some(error) = scope.pop().await {
            return Err(gpu(error));
        }
        let input = SharedFrames::new(device.clone(), decoder, width, height)?;
        let pipeline = Pipeline::new(device.clone(), queue.clone(), width, height)
            .await
            .map_err(enhancement)?;
        Ok(Self {
            device,
            queue,
            decoder: decoder.clone(),
            input,
            pipeline,
            blit,
            sampler,
            outputs: Vec::new(),
            output_size: (0, 0),
            busy: Arc::new(AtomicBool::new(false)),
            completed: Arc::new(Mutex::new(None)),
            failure,
            color: None,
            output_format,
        })
    }

    pub fn poll(&self) -> Result<Option<CompletedFrame>, VideoError> {
        self.device.poll(wgpu::PollType::Poll).map_err(gpu)?;
        if let Some(error) = self
            .failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            return Err(gpu(error));
        }
        Ok(self
            .completed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take())
    }

    pub(crate) fn busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
            || self.input.busy()
            || self.outputs.iter().any(|slot| slot.gpu_busy())
    }

    /// Copy only while holding the raw frame/metadata owner's short lock. Release
    /// that lock before calling asynchronous `render`; no lock spans compilation.
    pub fn capture(
        &mut self,
        raw: &ID3D11Texture2D,
        metadata: FrameMetadata,
    ) -> Result<Option<FrameLease>, VideoError> {
        if self.busy.load(Ordering::Acquire) {
            return Ok(None);
        }
        self.input.copy_frame(raw, metadata)
    }

    /// The captured lease is immutable throughout asynchronous shader preparation.
    pub async fn render(
        &mut self,
        input: FrameLease,
        color: Arc<ColorTransform>,
        options: Enhancement,
        epoch: u64,
        valid: impl Fn() -> bool,
    ) -> Result<bool, VideoError> {
        let (mut input, mut output) = (Some(input), None);
        let result = self
            .render_frame(&mut input, &mut output, color, options, epoch, valid)
            .await;
        // Even preparation/encoding failure must release the imported resources
        // through an ordered COMMON transition and a completed queue callback.
        if let Some(output) = output {
            output.discard(&self.device, &self.queue);
        }
        if let Some(input) = input {
            input.submit(
                self.device.create_command_encoder(&Default::default()),
                &self.queue,
            );
        }
        result
    }

    async fn render_frame(
        &mut self,
        input: &mut Option<FrameLease>,
        output: &mut Option<OutputLease>,
        color: Arc<ColorTransform>,
        options: Enhancement,
        epoch: u64,
        valid: impl Fn() -> bool,
    ) -> Result<bool, VideoError> {
        self.pipeline.prepare(options).await.map_err(enhancement)?;
        self.prepare_color(color);
        if !valid() {
            return Ok(false);
        }
        let metadata = input.as_ref().expect("captured input").metadata();
        let factor = if options.anime4k_on { 2 } else { 1 };
        let size = (
            metadata
                .width
                .checked_mul(factor)
                .ok_or_else(|| gpu("output width overflow"))?,
            metadata
                .height
                .checked_mul(factor)
                .ok_or_else(|| gpu("output height overflow"))?,
        );
        if self.output_size != size {
            self.outputs = vec![
                OutputSlot::new(
                    self.device.clone(),
                    &self.decoder,
                    size.0,
                    size.1,
                    self.output_format,
                )?,
                OutputSlot::new(
                    self.device.clone(),
                    &self.decoder,
                    size.0,
                    size.1,
                    self.output_format,
                )?,
            ];
            self.output_size = size;
        }
        for slot in &self.outputs {
            if let Some(lease) = slot.acquire()? {
                *output = Some(lease);
                break;
            }
        }
        if output.is_none() {
            return Ok(false);
        }
        let started = Instant::now();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let processed = self
            .pipeline
            .encode(
                &mut encoder,
                &input
                    .as_ref()
                    .expect("captured input")
                    .texture()
                    .create_view(&Default::default()),
            )
            .map_err(enhancement)?;
        let bindings = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("video-shared-output"),
            layout: &self.blit.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &processed.texture().create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self
                        .color
                        .as_ref()
                        .expect("prepared color")
                        .uniform
                        .as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(
                        &self
                            .color
                            .as_ref()
                            .expect("prepared color")
                            .lut
                            .create_view(&Default::default()),
                    ),
                },
            ],
        });
        let target = output
            .as_ref()
            .expect("acquired output")
            .texture()
            .create_view(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("video-to-d2d-scrgb"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
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
            pass.set_pipeline(&self.blit);
            pass.set_bind_group(0, &bindings, &[]);
            pass.draw(0..3, 0..1);
        }
        if !valid() {
            return Ok(false);
        }
        self.busy.store(true, Ordering::Release);
        let busy = self.busy.clone();
        let completed = self.completed.clone();
        output.take().expect("acquired output").submit(
            input.take().expect("captured input"),
            metadata,
            encoder,
            &self.queue,
            move |frame| {
                *completed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(CompletedFrame {
                    epoch,
                    elapsed: started.elapsed(),
                    frame,
                });
                busy.store(false, Ordering::Release);
            },
        );
        Ok(true)
    }

    fn prepare_color(&mut self, transform: Arc<ColorTransform>) {
        if self
            .color
            .as_ref()
            .is_some_and(|old| Arc::ptr_eq(&old.transform, &transform))
        {
            return;
        }
        let bytes = transform.params.bytes();
        let uniform = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("final-video-color"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&uniform, 0, &bytes);
        let lut = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("final-video-color-lut"),
            size: wgpu::Extent3d {
                width: LUT_WIDTH,
                height: LUT_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
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
        self.color = Some(FinalColor {
            transform,
            uniform,
            lut,
        });
    }
}

fn gpu(error: impl std::fmt::Display) -> VideoError {
    VideoError::new(ErrorCode::GpuFailed, error.to_string())
}
fn enhancement(error: hongguo_enhancement::Error) -> VideoError {
    let code = if matches!(error, hongguo_enhancement::Error::InputTooLarge) {
        ErrorCode::InputTooLarge
    } else {
        ErrorCode::GpuFailed
    };
    VideoError::new(code, error.to_string())
}

use crate::{
    cnn::Model,
    error::{Error, checked},
    texture::texture,
};
use hongguo_nsh_video_api::Enhancement;
use std::{borrow::Cow, sync::Arc};

const MAX_INPUT_PIXELS: u64 = 1920 * 1080;
const WORKGROUP: u32 = 8;
const MAX_STRENGTH: u8 = 100;
const MAX_SHARPEN_AMOUNT: f32 = 0.7;

pub struct FrameOutput {
    texture: wgpu::Texture,
}

impl FrameOutput {
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
    pub fn width(&self) -> u32 {
        self.texture.width()
    }
    pub fn height(&self) -> u32 {
        self.texture.height()
    }
}

pub struct Pipeline {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    normalized: wgpu::Texture,
    converted: wgpu::ComputePipeline,
    linearize: wgpu::ComputePipeline,
    sharpen: wgpu::ComputePipeline,
    params: wgpu::Buffer,
    cnn: Option<Model>,
    sharpened: Option<wgpu::Texture>,
    linearized: Option<wgpu::Texture>,
}

impl Pipeline {
    pub async fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        width: u32,
        height: u32,
    ) -> Result<Self, Error> {
        let limit = device.limits().max_texture_dimension_2d;
        if width == 0 || height == 0 || width > limit || height > limit {
            return Err(Error::InvalidSize);
        }
        checked(&device, || {
            let normalized = texture(&device, width, height, "video-normalized");
            let converted = compute(
                &device,
                include_str!("../shaders/convert.wgsl"),
                "video-normalize",
            );
            let sharpen = compute(
                &device,
                include_str!("../shaders/sharpen.wgsl"),
                "video-sharpen",
            );
            let linearize = compute(
                &device,
                include_str!("../shaders/linearize.wgsl"),
                "video-linearize",
            );
            let params = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sharpen-parameters"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            Ok(Self {
                device: device.clone(),
                queue,
                normalized,
                converted,
                linearize,
                sharpen,
                params,
                cnn: None,
                sharpened: None,
                linearized: None,
            })
        })
        .await
    }

    pub fn input_size(&self) -> (u32, u32) {
        (self.normalized.width(), self.normalized.height())
    }

    pub async fn prepare(&mut self, options: Enhancement) -> Result<(), Error> {
        if options.strength > MAX_STRENGTH {
            return Err(Error::InvalidStrength);
        }
        let (width, height) = self.input_size();
        if options.anime4k_on {
            if u64::from(width) * u64::from(height) > MAX_INPUT_PIXELS {
                return Err(Error::InputTooLarge);
            }
            if self.cnn.is_none() {
                self.cnn = Some(Model::new(self.device.clone(), width, height).await?);
            }
        } else {
            self.cnn = None;
        }
        let scale = if options.anime4k_on { 2 } else { 1 };
        let output_size = (width * scale, height * scale);
        if options.anime4k_on {
            if self.linearized.is_none() {
                self.linearized = Some(
                    checked(&self.device, || {
                        Ok(texture(
                            &self.device,
                            output_size.0,
                            output_size.1,
                            "video-cnn-linear",
                        ))
                    })
                    .await?,
                );
            }
        } else {
            self.linearized = None;
        }
        if options.sharpen_on {
            if self
                .sharpened
                .as_ref()
                .is_none_or(|target| (target.width(), target.height()) != output_size)
            {
                self.sharpened = Some(
                    checked(&self.device, || {
                        Ok(texture(
                            &self.device,
                            output_size.0,
                            output_size.1,
                            "video-sharpened",
                        ))
                    })
                    .await?,
                );
            }
        } else {
            self.sharpened = None;
        }
        let values = [
            f32::from(options.strength) / f32::from(MAX_STRENGTH) * MAX_SHARPEN_AMOUNT,
            if options.anime4k_on { 1.0 } else { 0.0 },
            0.0,
            0.0,
        ];
        let bytes = values
            .into_iter()
            .flat_map(f32::to_ne_bytes)
            .collect::<Vec<_>>();
        self.queue.write_buffer(&self.params, 0, &bytes);
        Ok(())
    }

    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::TextureView,
    ) -> Result<FrameOutput, Error> {
        self.pass(encoder, &self.converted, input, &self.normalized, true);
        let base = self.cnn.as_ref().map_or_else(
            || self.normalized.clone(),
            |model| model.encode(encoder, &self.normalized.create_view(&Default::default())),
        );
        let base = if let Some(linearized) = &self.linearized {
            self.pass(
                encoder,
                &self.linearize,
                &base.create_view(&Default::default()),
                linearized,
                false,
            );
            linearized.clone()
        } else {
            base
        };
        if let Some(target) = &self.sharpened {
            self.pass(
                encoder,
                &self.sharpen,
                &base.create_view(&Default::default()),
                target,
                true,
            );
            Ok(FrameOutput {
                texture: target.clone(),
            })
        } else {
            Ok(FrameOutput { texture: base })
        }
    }

    fn pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        input: &wgpu::TextureView,
        target: &wgpu::Texture,
        sharpen: bool,
    ) {
        let output = target.create_view(&Default::default());
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(input),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&output),
            },
        ];
        if sharpen {
            entries.push(wgpu::BindGroupEntry {
                binding: 2,
                resource: self.params.as_entire_binding(),
            });
        }
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("enhancement-frame"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(
            target.width().div_ceil(WORKGROUP),
            target.height().div_ceil(WORKGROUP),
            1,
        );
    }
}

fn compute(
    device: &wgpu::Device,
    source: &'static str,
    label: &'static str,
) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(source)),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    })
}

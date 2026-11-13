use crate::{
    error::{Error, checked},
    model::{Binding, Graph, Pass, SHADERS},
    texture::texture,
};
use std::{borrow::Cow, sync::Arc};

enum Pipeline {
    Compute(wgpu::ComputePipeline),
    Render(wgpu::RenderPipeline),
}

struct Step {
    pipeline: Pipeline,
    layout: wgpu::BindGroupLayout,
    bindings: Vec<Binding>,
    output: usize,
    workgroup: [u32; 2],
    vertices: u32,
}

pub(crate) struct Model {
    device: Arc<wgpu::Device>,
    textures: Vec<wgpu::Texture>,
    sampler: wgpu::Sampler,
    steps: Vec<Step>,
    output: usize,
}

impl Model {
    pub async fn new(device: Arc<wgpu::Device>, width: u32, height: u32) -> Result<Self, Error> {
        checked(&device, || Self::build(device.clone(), width, height)).await
    }

    fn build(device: Arc<wgpu::Device>, width: u32, height: u32) -> Result<Self, Error> {
        let graph = Graph::load().map_err(|error| Error::Model(error.to_string()))?;
        if graph
            .passes
            .iter()
            .flat_map(Pass::bindings)
            .any(|binding| binding.texture.is_none() && binding.sampler != Some(true))
        {
            return Err(Error::Model(
                "binding is neither a texture nor a sampler".into(),
            ));
        }
        let textures = graph
            .textures
            .iter()
            .skip(1)
            .map(|spec| {
                texture(
                    &device,
                    width * spec.scale[0],
                    height * spec.scale[1],
                    "anime4k-intermediate",
                )
            })
            .collect();
        let shaders = SHADERS
            .iter()
            .map(|source| {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("anime4k-cnnx2m"),
                    source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(source)),
                })
            })
            .collect::<Vec<_>>();
        let steps = graph
            .passes
            .into_iter()
            .map(|pass| Step::new(&device, &shaders, pass))
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Ok(Self {
            device,
            textures,
            sampler,
            steps,
            output: graph.output,
        })
    }

    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::TextureView,
    ) -> wgpu::Texture {
        let mut views = vec![input.clone()];
        views.extend(
            self.textures
                .iter()
                .map(|texture| texture.create_view(&Default::default())),
        );
        for step in &self.steps {
            let entries = step
                .bindings
                .iter()
                .map(|binding| wgpu::BindGroupEntry {
                    binding: binding.binding,
                    resource: if let Some(texture) = binding.texture {
                        wgpu::BindingResource::TextureView(&views[texture])
                    } else {
                        wgpu::BindingResource::Sampler(&self.sampler)
                    },
                })
                .collect::<Vec<_>>();
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("anime4k-bindings"),
                layout: &step.layout,
                entries: &entries,
            });
            let target = &self.textures[step.output - 1];
            match &step.pipeline {
                Pipeline::Compute(pipeline) => {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &group, &[]);
                    pass.dispatch_workgroups(
                        target.width().div_ceil(step.workgroup[0]),
                        target.height().div_ceil(step.workgroup[1]),
                        1,
                    );
                }
                Pipeline::Render(pipeline) => {
                    let attachments = [Some(wgpu::RenderPassColorAttachment {
                        view: &views[step.output],
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })];
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &attachments,
                        ..Default::default()
                    });
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &group, &[]);
                    pass.draw(0..step.vertices, 0..1);
                }
            }
        }
        self.textures[self.output - 1].clone()
    }
}

impl Step {
    fn new(device: &wgpu::Device, shaders: &[wgpu::ShaderModule], spec: Pass) -> Self {
        match spec {
            Pass::Compute {
                shader,
                entry,
                bindings,
                workgroup,
                output,
            } => {
                let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("anime4k-convolution"),
                    layout: None,
                    module: &shaders[shader],
                    entry_point: Some(&entry),
                    compilation_options: Default::default(),
                    cache: None,
                });
                let layout = pipeline.get_bind_group_layout(0);
                Self {
                    pipeline: Pipeline::Compute(pipeline),
                    layout,
                    bindings,
                    output,
                    workgroup,
                    vertices: 0,
                }
            }
            Pass::Render {
                vertex,
                vertex_entry,
                fragment,
                fragment_entry,
                bindings,
                vertices,
                output,
            } => {
                let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("anime4k-residual-overlay"),
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: &shaders[vertex],
                        entry_point: Some(&vertex_entry),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shaders[fragment],
                        entry_point: Some(&fragment_entry),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba16Float,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                });
                let layout = pipeline.get_bind_group_layout(0);
                Self {
                    pipeline: Pipeline::Render(pipeline),
                    layout,
                    bindings,
                    output,
                    workgroup: [1, 1],
                    vertices,
                }
            }
        }
    }
}

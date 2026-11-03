//! Particles over the finished scene: decals, sprites and streaks, drawn after
//! the scene and before it is read back. They cover whole pixels, so they draw
//! into the finished colour, with no samples, and read the scene's depth through
//! a texture to test it as software does.

use mscent_math::epsilon::DECAL_DEPTH_BIAS;
use mscent_render::Camera;
use mscent_render::particles::{ParticleDraws, SpriteBank};

use crate::device::{Gpu, GpuError};
use crate::faces::storage;
use crate::finish::Finish;
use crate::particle_data::{CORNER_BYTES, DECAL_BYTES, Listed, SPRITE_BYTES};
use crate::resolve::DepthResolve;
use crate::targets::{COLOR, Targets};

/// `size` and `depth_terms` of `Particles` in `particles.wgsl`.
const UNIFORM_BYTES: u64 = 16;
/// How the scene's depth is read: a depth texture as it is, or a multisampled
/// one already resolved to one value a pixel.
const DEPTH_READ: &str = "@group(0) @binding(5) var scene: texture_depth_2d;
fn scene_depth(at: vec2u) -> f32 { return textureLoad(scene, at, 0); }
";
const RESOLVED_READ: &str = "@group(0) @binding(5) var scene: texture_2d<f32>;
fn scene_depth(at: vec2u) -> f32 { return textureLoad(scene, at, 0).r; }
";

/// What one frame's particles were put in, to bind them.
struct Uploaded {
    sprites: wgpu::Buffer,
    decals: wgpu::Buffer,
    corners: wgpu::Buffer,
}

pub(crate) struct ParticlePass {
    layout: wgpu::BindGroupLayout,
    over: wgpu::RenderPipeline,
    add: wgpu::RenderPipeline,
    decal: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    /// The sprite masks, taken with the first frame that has particles.
    bank: Option<wgpu::Buffer>,
    listed: Listed,
    uploaded: Option<Uploaded>,
}

fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn layout(gpu: &Gpu, finish: Finish) -> wgpu::BindGroupLayout {
    let sample_type = if finish.samples() > 1 {
        wgpu::TextureSampleType::Float { filterable: false }
    } else {
        wgpu::TextureSampleType::Depth
    };
    let uniform = wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let depth = wgpu::BindGroupLayoutEntry {
        binding: 5,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let entries = [
        uniform,
        storage_entry(1),
        storage_entry(2),
        storage_entry(3),
    ];
    let entries: Vec<_> = entries
        .into_iter()
        .chain([storage_entry(4), depth])
        .collect();
    let descriptor = wgpu::BindGroupLayoutDescriptor {
        label: Some("particles"),
        entries: &entries,
    };
    gpu.device.create_bind_group_layout(&descriptor)
}

/// Colour `src * a + dst * b`, the alpha kept as it is.
fn blend(src: wgpu::BlendFactor, dst: wgpu::BlendFactor) -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: src,
            dst_factor: dst,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

struct Stage<'a> {
    module: &'a wgpu::ShaderModule,
    layout: &'a wgpu::PipelineLayout,
}

fn pipeline(
    gpu: &Gpu,
    stage: &Stage,
    vs: &str,
    fs: &str,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    let constants = [("DECAL_DEPTH_BIAS", f64::from(DECAL_DEPTH_BIAS))];
    let options = wgpu::PipelineCompilationOptions {
        constants: &constants,
        ..Default::default()
    };
    gpu.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(fs),
            layout: Some(stage.layout),
            vertex: wgpu::VertexState {
                module: stage.module,
                entry_point: Some(vs),
                compilation_options: options.clone(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: stage.module,
                entry_point: Some(fs),
                compilation_options: options,
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
}

impl ParticlePass {
    pub(crate) fn new(gpu: &Gpu, finish: Finish) -> Self {
        let read = if finish.samples() > 1 {
            RESOLVED_READ
        } else {
            DEPTH_READ
        };
        let source = format!("{read}{}", include_str!("../shaders/particles.wgsl"));
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("particles"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let layout = layout(gpu, finish);
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("particles"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let stage = Stage {
            module: &module,
            layout: &pipeline_layout,
        };
        use wgpu::BlendFactor::{One, OneMinusSrcAlpha, Src, Zero};
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particles"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            over: pipeline(
                gpu,
                &stage,
                "vs_sprite",
                "fs_over",
                blend(One, OneMinusSrcAlpha),
            ),
            add: pipeline(gpu, &stage, "vs_sprite", "fs_add", blend(One, One)),
            decal: pipeline(gpu, &stage, "vs_decal", "fs_decal", blend(Zero, Src)),
            layout,
            uniform,
            bank: None,
            listed: Listed::default(),
            uploaded: None,
        }
    }

    /// Lists this frame's particles and puts them on the GPU. False when there
    /// is nothing to draw.
    pub(crate) fn prepare(
        &mut self,
        gpu: &Gpu,
        cam: &Camera,
        particles: Option<(&ParticleDraws, &SpriteBank)>,
    ) -> Result<bool, GpuError> {
        self.uploaded = None;
        let Some((draws, bank)) = particles else {
            return Ok(false);
        };
        let size = (cam.width(), cam.height());
        self.listed.list(draws, size);
        if self.listed.is_empty() {
            return Ok(false);
        }
        if self.bank.is_none() {
            self.bank = Some(storage(gpu, "sprite masks", bank.bytes(), 4)?);
        }
        let (a, k) = cam.gpu_depth_terms();
        let values = [size.0 as f32, size.1 as f32, a, k];
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        gpu.queue.write_buffer(&self.uniform, 0, &bytes);
        let l = &self.listed;
        self.uploaded = Some(Uploaded {
            sprites: storage(gpu, "sprites", &l.sprites, SPRITE_BYTES)?,
            decals: storage(gpu, "decals", &l.decals, DECAL_BYTES)?,
            corners: storage(gpu, "decal corners", &l.corners, CORNER_BYTES)?,
        });
        Ok(true)
    }

    /// Draws what [`Self::prepare`] put on the GPU over `targets`' colour.
    pub(crate) fn draw(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        resolve: Option<&DepthResolve>,
    ) {
        let (Some(up), Some(bank)) = (&self.uploaded, &self.bank) else {
            return;
        };
        let depth = targets.pixel_depth(encoder, resolve);
        let buffers = [&self.uniform, bank, &up.sprites, &up.decals, &up.corners];
        let mut entries: Vec<_> = (0u32..)
            .zip(buffers)
            .map(|(binding, b)| wgpu::BindGroupEntry {
                binding,
                resource: b.as_entire_binding(),
            })
            .collect();
        entries.push(wgpu::BindGroupEntry {
            binding: 5,
            resource: wgpu::BindingResource::TextureView(depth),
        });
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particles"),
            layout: &self.layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("particles"),
            color_attachments: &[Some(targets.finished_attachment())],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &group, &[]);
        let corners = self.listed.decal_corners();
        if corners > 0 {
            pass.set_pipeline(&self.decal);
            pass.draw(0..corners, 0..1);
        }
        for (pipeline, instances) in [
            (&self.over, &self.listed.over),
            (&self.add, &self.listed.add),
        ] {
            if !instances.is_empty() {
                pass.set_pipeline(pipeline);
                pass.draw(0..6, instances.clone());
            }
        }
    }
}

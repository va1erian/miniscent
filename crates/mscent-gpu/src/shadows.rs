//! Projected shadows from the frame's dynamic lights (`docs/hardware-effects.md`).
//!
//! The strongest `SHADOW_LIGHTS` lights each get a cube map, drawn before the scene
//! from what stands in their way, as six passes. A face of it holds the distance
//! from the light over its reach, so the faces' shader looks a point up by its
//! direction alone (`shadow_sample.wgsl`). This pass is in this frame only at High
//! with a light that casts, and costs nothing otherwise.
//!
//! Casters in this slice are enemy models, actors and movers. Static faces and
//! voxel chunks join by adding a list to [`Casters`] and a draw to
//! [`ShadowPass::draw_casters`]; nothing else here changes.

use std::ops::Range;
use std::sync::Arc;

use mscent_math::Vec3;

use crate::device::Gpu;
use crate::shadow_lights::{Candidate, FACES, Pick, SHADOW_LIGHTS, Slots, cube_bytes};
use crate::targets::DEPTH;

/// A side of a cube face, in texels.
pub(crate) const SIDE: u32 = 768;
/// Beyond the map, which is lit.
const NOTHING: f64 = 2.0;
const MAP: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
const LAYERS: u32 = 6 * SHADOW_LIGHTS as u32;
/// The nearest a caster is drawn to the light, in world units.
const NEAR: f32 = 0.05;
/// Bytes of a `Cube` in `shadow.wgsl`.
const CUBE_BYTES: u64 = 64;

// What the sampling in `shadow_sample.wgsl` is tuned with. Look settings, as
// `BUMP_FULL` is, and not tolerances.
/// Moves a receiver off its plane along its normal, in texels of the map.
const NORMAL_OFFSET: f32 = 1.5;
/// A constant and a slope-scaled term of bias, in texels.
const BIAS: f32 = 0.75;
const SLOPE_BIAS: f32 = 0.5;
/// How big the light looks, in world units: how wide a penumbra grows with the
/// receiver's distance from what blocks it.
const LIGHT_SIZE: f32 = 0.5;
/// The narrowest and the widest kernel of the filter, in texels.
const MIN_KERNEL: f32 = 2.0;
const MAX_KERNEL: f32 = 16.0;
const BLOCKER_TAPS: u32 = 8;
const TAPS: u32 = 24;

/// The sampling's overrides, for the faces' shader.
pub(crate) fn sample_constants() -> [(&'static str, f64); 10] {
    [
        ("SHADOW_LIGHTS", SHADOW_LIGHTS as f64),
        ("SHADOW_SIDE", f64::from(SIDE)),
        ("SHADOW_NORMAL_OFFSET", f64::from(NORMAL_OFFSET)),
        ("SHADOW_BIAS", f64::from(BIAS)),
        ("SHADOW_SLOPE_BIAS", f64::from(SLOPE_BIAS)),
        ("SHADOW_LIGHT_SIZE", f64::from(LIGHT_SIZE)),
        ("SHADOW_MIN_KERNEL", f64::from(MIN_KERNEL)),
        ("SHADOW_MAX_KERNEL", f64::from(MAX_KERNEL)),
        ("SHADOW_BLOCKER_TAPS", f64::from(BLOCKER_TAPS)),
        ("SHADOW_TAPS", f64::from(TAPS)),
    ]
}

/// The cube maps, one array of them, and what the faces' shader reads them with.
pub(crate) struct ShadowMaps {
    /// Whether this GPU can hold them. One that cannot is given maps of a texel,
    /// which the shader never reads, and draws no shadows.
    ready: bool,
    pub(crate) cube: wgpu::TextureView,
    pub(crate) sampler: wgpu::Sampler,
    /// A view of each layer, to draw into.
    layers: Vec<wgpu::TextureView>,
    depth: wgpu::TextureView,
}

impl ShadowMaps {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let limits = device.limits();
        let ready =
            limits.max_texture_dimension_2d >= SIDE && limits.max_texture_array_layers >= LAYERS;
        let side = if ready { SIDE } else { 1 };
        let make = |format, layers, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shadow maps"),
                size: wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let drawn = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let maps = make(MAP, LAYERS, drawn | wgpu::TextureUsages::TEXTURE_BINDING);
        let cube = maps.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::CubeArray),
            ..Default::default()
        });
        let layers = (0..LAYERS)
            .map(|layer| {
                maps.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let depth = make(DEPTH, 1, drawn).create_view(&Default::default());
        // Taps are single texels: the filter is the shader's own.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Self {
            ready,
            cube,
            sampler,
            layers,
            depth,
        }
    }
}

/// What a model part contributes: where its triangles are in the corner list, and
/// a sphere in the world that holds it.
pub(crate) struct PartSphere {
    pub(crate) corners: Range<u32>,
    pub(crate) centre: Vec3,
    pub(crate) radius: f32,
}

/// Enemy models' parts: storage the voxel pipeline reads, and a matrix a part.
pub(crate) struct ModelCasters<'a> {
    pub(crate) vertices: &'a wgpu::Buffer,
    pub(crate) corners: &'a wgpu::Buffer,
    pub(crate) worlds: &'a wgpu::Buffer,
    pub(crate) parts: &'a [PartSphere],
}

/// One mover's triangles, in the level's index list for movers, and a sphere in
/// the world that holds them where the mover now is.
pub(crate) struct MoverDraw {
    pub(crate) pose: u32,
    pub(crate) indices: Range<u32>,
    pub(crate) centre: Vec3,
    pub(crate) radius: f32,
}

/// Movers: the faces' vertices, as authored, and their poses.
pub(crate) struct MoverCasters<'a> {
    pub(crate) vertices: &'a wgpu::Buffer,
    pub(crate) indices: &'a wgpu::Buffer,
    pub(crate) poses: &'a wgpu::Buffer,
    pub(crate) draws: &'a [MoverDraw],
}

/// Everything a frame has that casts a shadow.
#[derive(Default)]
pub(crate) struct Casters<'a> {
    /// Actors' vertices, and how many.
    pub(crate) actors: Option<(&'a wgpu::Buffer, u32)>,
    pub(crate) models: Option<ModelCasters<'a>>,
    pub(crate) movers: Option<MoverCasters<'a>>,
}

/// The caster pipelines, compiled once for a device.
pub(crate) struct Pipelines {
    cube: wgpu::BindGroupLayout,
    world: wgpu::RenderPipeline,
    model: wgpu::RenderPipeline,
    mover: wgpu::RenderPipeline,
}

const SHADER: &str = include_str!("../shaders/shadow.wgsl");

fn storage(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

impl Pipelines {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadows"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let cube = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow cube"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let group = |label, bindings: &[u32]| {
            let entries: Vec<_> = bindings.iter().map(|&b| storage(b)).collect();
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &entries,
            })
        };
        let models = group("shadow models", &[0, 1, 2]);
        let movers = group("shadow movers", &[3]);
        let layout = |extra: Option<&wgpu::BindGroupLayout>| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadows"),
                bind_group_layouts: &[Some(&cube), extra],
                immediate_size: 0,
            })
        };
        let constants = [("SHADOW_NEAR", f64::from(NEAR))];
        let make = |entry, layout: &wgpu::PipelineLayout, stride: Option<u64>| {
            // Triangles of actors and movers come with a vertex buffer, the models'
            // are pulled.
            let attributes = wgpu::vertex_attr_array![0 => Float32x3];
            let buffers: Vec<_> = stride
                .into_iter()
                .map(|array_stride| {
                    Some(wgpu::VertexBufferLayout {
                        array_stride,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    })
                })
                .collect();
            let options = wgpu::PipelineCompilationOptions {
                constants: &constants,
                ..Default::default()
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: options.clone(),
                    buffers: &buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: options,
                    targets: &[Some(MAP.into())],
                }),
                // Both sides cast: a thin plate or an open actor shades what is
                // behind it from either side.
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            world: make("vs_world", &layout(None), Some(crate::actors::VERTEX_BYTES)),
            model: make("vs_model", &layout(Some(&models)), None),
            mover: make(
                "vs_mover",
                &layout(Some(&movers)),
                Some(crate::face_pipeline::VERTEX_BYTES as u64),
            ),
            cube,
        }
    }
}

/// The shadow maps of a renderer, and who holds them.
pub(crate) struct ShadowPass {
    pub(crate) maps: ShadowMaps,
    pipelines: Arc<Pipelines>,
    uniform: wgpu::Buffer,
    /// By face of each map: its `Cube`.
    cubes: Vec<wgpu::BindGroup>,
    stride: u64,
    pick: Pick,
}

/// Whether two spheres touch.
fn meet(a: (Vec3, f32), b: (Vec3, f32)) -> bool {
    (a.0 - b.0).length() <= a.1 + b.1
}

impl ShadowPass {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        let pipelines = gpu
            .shadow_pipelines
            .get_or_init(|| Arc::new(Pipelines::new(gpu)))
            .clone();
        let align = u64::from(gpu.device.limits().min_uniform_buffer_offset_alignment);
        let stride = CUBE_BYTES.next_multiple_of(align.max(1));
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow cubes"),
            size: stride * u64::from(LAYERS),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cubes = (0..u64::from(LAYERS))
            .map(|layer| {
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("shadow cube"),
                    layout: &pipelines.cube,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &uniform,
                            offset: layer * stride,
                            size: wgpu::BufferSize::new(CUBE_BYTES),
                        }),
                    }],
                })
            })
            .collect();
        Self {
            maps: ShadowMaps::new(gpu),
            pipelines,
            uniform,
            cubes,
            stride,
            pick: Pick::default(),
        }
    }

    /// Which lights hold maps this frame, as the `Frame` wants them, and who they
    /// are. `eye` is the camera's.
    pub(crate) fn pick(
        &mut self,
        lights: &[mscent_render::DynLight],
        eye: Vec3,
    ) -> (Slots, [Option<(usize, Candidate)>; SHADOW_LIGHTS]) {
        if !self.maps.ready {
            return (Slots::NONE, Default::default());
        }
        self.pick.pick(lights, eye)
    }

    /// Draws the maps of `holders` from `casters`, ahead of the scene.
    pub(crate) fn encode(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        holders: &[Option<(usize, Candidate)>; SHADOW_LIGHTS],
        casters: &Casters,
    ) {
        let groups = self.caster_groups(gpu, casters);
        for (slot, holder) in holders.iter().enumerate() {
            let Some((_, light)) = holder else { continue };
            for face in 0..FACES.len() {
                let layer = slot * FACES.len() + face;
                let cube = cube_bytes(light.pos, light.radius, face);
                gpu.queue
                    .write_buffer(&self.uniform, layer as u64 * self.stride, &cube);
                let mut pass = self.begin(encoder, layer);
                pass.set_bind_group(0, &self.cubes[layer], &[]);
                self.draw_casters(&mut pass, light, casters, &groups);
            }
        }
    }

    fn begin<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        layer: usize,
    ) -> wgpu::RenderPass<'e> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadow face"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.maps.layers[layer],
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: NOTHING,
                        ..wgpu::Color::BLACK
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.maps.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    /// The bind groups of the models' and movers' storage, once for the frame.
    fn caster_groups(
        &self,
        gpu: &Gpu,
        casters: &Casters,
    ) -> (Option<wgpu::BindGroup>, Option<wgpu::BindGroup>) {
        let p = &self.pipelines;
        let make = |pipeline: &wgpu::RenderPipeline, buffers: &[(u32, &wgpu::Buffer)]| {
            let entries: Vec<_> = buffers
                .iter()
                .map(|&(binding, buffer)| wgpu::BindGroupEntry {
                    binding,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shadow casters"),
                layout: &pipeline.get_bind_group_layout(1),
                entries: &entries,
            })
        };
        let models = casters
            .models
            .as_ref()
            .map(|m| make(&p.model, &[(0, m.vertices), (1, m.corners), (2, m.worlds)]));
        let movers = casters
            .movers
            .as_ref()
            .map(|m| make(&p.mover, &[(3, m.poses)]));
        (models, movers)
    }

    /// Draws what of `casters` meets `light`'s sphere into the pass's face.
    fn draw_casters(
        &self,
        pass: &mut wgpu::RenderPass,
        light: &Candidate,
        casters: &Casters,
        (models, movers): &(Option<wgpu::BindGroup>, Option<wgpu::BindGroup>),
    ) {
        let reach = (light.pos, light.radius);
        let p = &self.pipelines;
        // Actors are few, and their triangles come in one list: all of them.
        if let Some((vertices, count)) = casters.actors.filter(|&(_, n)| n > 0) {
            pass.set_pipeline(&p.world);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.draw(0..count, 0..1);
        }
        if let (Some(m), Some(group)) = (&casters.models, models) {
            pass.set_pipeline(&p.model);
            pass.set_bind_group(1, group, &[]);
            for (part, sphere) in (0u32..).zip(m.parts) {
                let near = meet(reach, (sphere.centre, sphere.radius));
                if near && !sphere.corners.is_empty() {
                    pass.draw(sphere.corners.clone(), part..part + 1);
                }
            }
        }
        if let (Some(m), Some(group)) = (&casters.movers, movers) {
            pass.set_pipeline(&p.mover);
            pass.set_bind_group(1, group, &[]);
            pass.set_vertex_buffer(0, m.vertices.slice(..));
            pass.set_index_buffer(m.indices.slice(..), wgpu::IndexFormat::Uint32);
            for draw in m.draws {
                if meet(reach, (draw.centre, draw.radius)) && !draw.indices.is_empty() {
                    pass.draw_indexed(draw.indices.clone(), 0, draw.pose..draw.pose + 1);
                }
            }
        }
    }
}

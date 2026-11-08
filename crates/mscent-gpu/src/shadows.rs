//! Projected shadows from the frame's dynamic lights and the level's lamps
//! (`docs/hardware-effects.md`).
//!
//! The strongest `SHADOW_LIGHTS` lights, and `LAMP_SHADOWS` static lamps
//! (`lamp_shadows.rs`), each get a cube map, drawn before the scene
//! from what stands in their way, as six passes. A face of it holds the distance
//! from the light over its reach, so the faces' shader looks a point up by its
//! direction alone (`shadow_sample.wgsl`). This pass is in this frame only at High
//! with a light that casts, and costs nothing otherwise.
//!
//! Casters in this slice are enemy models, actors and movers. Static faces and
//! voxel chunks join by adding a list to [`Casters`] and a draw to
//! [`ShadowPass::draw_casters`]; nothing else here changes.

use std::sync::Arc;

use mscent_math::Vec3;
use mscent_world::lightmap::PointLight;

use crate::device::{Gpu, GpuError};
use crate::lamp_shadows::{FALLOFF_POWER, LAMP_SHADOWS, LampPick, Lamps, frame_lamps};
pub(crate) use crate::shadow_casters::{
    Casters, ModelCasters, MoverCasters, MoverDraw, PartSphere, meet,
};
use crate::shadow_lights::{Candidate, FACES, Pick, SHADOW_LIGHTS, Slots, cube_bytes};
use crate::targets::DEPTH;

use crate::shadow_maps::Drawable;
pub(crate) use crate::shadow_maps::{ShadowMaps, cube_arrays};

/// A side of a cube face, in texels.
pub(crate) const SIDE: u32 = 768;
/// Beyond the map, which is lit.
const NOTHING: f64 = 2.0;
pub(crate) const MAP: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
/// Maps in a frame: the dynamic lights', then the lamps'.
pub(crate) const MAPS: usize = SHADOW_LIGHTS + LAMP_SHADOWS;
pub(crate) const LAYERS: u32 = 6 * MAPS as u32;
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
pub(crate) fn sample_constants() -> [(&'static str, f64); 13] {
    [
        ("SHADOW_LIGHTS", SHADOW_LIGHTS as f64),
        ("SHADOW_MAPS", MAPS as f64),
        ("LAMP_SHADOWS", LAMP_SHADOWS as f64),
        ("LAMP_FALLOFF_POWER", FALLOFF_POWER),
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

/// What drawing a map needs beyond the maps, made with the first light that casts:
/// the caster pipelines, which a device compiles once, and a `Cube` a face.
struct Drawing {
    pipelines: Arc<Pipelines>,
    uniform: wgpu::Buffer,
    /// By face of each map: its `Cube`.
    cubes: Vec<wgpu::BindGroup>,
    stride: u64,
}

impl Drawing {
    fn new(gpu: &Gpu) -> Result<Self, GpuError> {
        let pipelines = gpu
            .shadow_pipelines
            .get_or_init(|| gpu.checked(|| Ok(Arc::new(Pipelines::new(gpu)))))
            .clone()?;
        gpu.checked(|| {
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
            Ok(Self {
                pipelines,
                uniform,
                cubes,
                stride,
            })
        })
    }
}

/// The shadow maps of a renderer, and who holds them.
pub(crate) struct ShadowPass {
    pub(crate) maps: ShadowMaps,
    /// Made when a light first casts. A failure is kept, and the renderer draws
    /// on without shadows, as it does where the maps do not fit.
    drawing: Option<Result<Drawing, ()>>,
    pick: Pick<SHADOW_LIGHTS>,
    lamp_pick: LampPick,
}

/// Which lights hold maps in a frame.
pub(crate) struct Picked {
    /// The map of each dynamic light.
    pub(crate) slots: Slots,
    /// The lamps that hold maps, as `Frame` has them.
    pub(crate) lamps: Lamps,
    /// By map, who holds it and where: the dynamic lights first, then the lamps.
    pub(crate) holders: [Option<(usize, Candidate)>; MAPS],
}

impl Picked {
    /// No map is held.
    pub(crate) const NONE: Self = Self {
        slots: Slots::NONE,
        lamps: Lamps([None; LAMP_SHADOWS]),
        holders: [None; MAPS],
    };

    pub(crate) fn any(&self) -> bool {
        self.holders.iter().any(Option::is_some)
    }
}

impl ShadowPass {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        Self {
            maps: ShadowMaps::new(gpu),
            drawing: None,
            pick: Pick::default(),
            lamp_pick: LampPick::default(),
        }
    }

    /// Whether this device holds maps at all.
    pub(crate) fn ready(&self) -> bool {
        self.maps.ready()
    }

    /// Which lights and lamps hold maps this frame, as the `Frame` wants them.
    /// `eye` is the camera's, and `casters` what a lamp must reach to cast.
    /// Compiles the caster pipelines the first time a map is held; if that fails,
    /// none is, and the error is reported.
    pub(crate) fn pick(
        &mut self,
        gpu: &Gpu,
        (lights, lamps): (&[mscent_render::DynLight], &[PointLight]),
        eye: Vec3,
        casters: &Casters,
    ) -> Picked {
        if !self.maps.ready() {
            return Picked::NONE;
        }
        let (slots, lit) = self.pick.pick(lights, eye);
        let held = self.lamp_pick.pick(lamps, eye, |p, r| casters.reach(p, r));
        let mut holders = [None; MAPS];
        holders[..SHADOW_LIGHTS].copy_from_slice(&lit);
        holders[SHADOW_LIGHTS..].copy_from_slice(&held);
        let picked = Picked {
            slots,
            lamps: frame_lamps(lamps, &held, |p, r| casters.bound(p, r)),
            holders,
        };
        if !picked.any() {
            return picked;
        }
        let drawing = self.drawing.get_or_insert_with(|| {
            Drawing::new(gpu).map_err(|why| gpu.report(format!("shadows are off: {why}")))
        });
        match drawing {
            Ok(_) => picked,
            Err(()) => Picked::NONE,
        }
    }

    /// Draws the maps of `holders` from `casters`, ahead of the scene.
    pub(crate) fn encode(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        holders: &[Option<(usize, Candidate)>; MAPS],
        casters: &Casters,
    ) {
        let (Some(Ok(drawing)), Some(drawable)) = (&self.drawing, &self.maps.drawable) else {
            return;
        };
        let groups = Self::caster_groups(&drawing.pipelines, gpu, casters);
        for (slot, holder) in holders.iter().enumerate() {
            let Some((_, light)) = holder else { continue };
            for face in 0..FACES.len() {
                let layer = slot * FACES.len() + face;
                let cube = cube_bytes(light.pos, light.radius, face);
                gpu.queue
                    .write_buffer(&drawing.uniform, layer as u64 * drawing.stride, &cube);
                let mut pass = Self::begin(drawable, encoder, layer);
                pass.set_bind_group(0, &drawing.cubes[layer], &[]);
                Self::draw_casters(&drawing.pipelines, &mut pass, light, casters, &groups);
            }
        }
    }

    fn begin<'e>(
        drawable: &Drawable,
        encoder: &'e mut wgpu::CommandEncoder,
        layer: usize,
    ) -> wgpu::RenderPass<'e> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadow face"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &drawable.layers[layer],
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
                view: &drawable.depth,
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
        p: &Pipelines,
        gpu: &Gpu,
        casters: &Casters,
    ) -> (Option<wgpu::BindGroup>, Option<wgpu::BindGroup>) {
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
        p: &Pipelines,
        pass: &mut wgpu::RenderPass,
        light: &Candidate,
        casters: &Casters,
        (models, movers): &(Option<wgpu::BindGroup>, Option<wgpu::BindGroup>),
    ) {
        let reach = (light.pos, light.radius);
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

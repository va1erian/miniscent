//! Voxel surfaces: each chunk's three levels of detail in buffers of its own, so a
//! remesh replaces only the chunks it made, and each frame one draw per chunk in
//! view, at the level the software renderer's own list chooses.

use std::ops::Range;

use mscent_render::bump::{MAX_GAIN, MIN_FLAT, MIN_GAIN, RELIEF};
use mscent_render::lighting_shader::{BUMP_FULL, BUMP_GONE};
use mscent_render::raster::{LONG_AXIS_LIMIT, SHARP_RATE};
use mscent_render::scene::{SceneStats, eye_of};
use mscent_render::texel_light::{KNEE, MIN_SPEC, ROLL, SPEC_GAIN};
use mscent_render::voxel::{
    AXES, BAYER, ChunkLook, IN_SEAM, LodLook, MATERIAL_BAND, SEAM_BAND, TEXELS_PER_UNIT, VoxelList,
    VoxelScene, spot_field,
};
use mscent_render::{Camera, DynLight, LevelAssets};

use crate::common::with_common;
use crate::device::{Gpu, GpuError};
use crate::dyn_lights;
use crate::face_data::TABLE_LEN;
use crate::faces::storage;
use crate::finish::Finish;
use crate::targets::{COLOR, DEPTH};
use crate::voxel_materials::{self, MATERIALS};

/// The `Voxels` uniform of `voxel.wgsl`.
pub(crate) const UNIFORM_BYTES: u64 = 192;
/// A `Vertex` of `voxel.wgsl`.
const VERTEX_BYTES: u64 = 64;
const SHADER: &str = concat!(
    include_str!("../shaders/common.wgsl"),
    include_str!("../shaders/shading.wgsl"),
    include_str!("../shaders/voxel.wgsl"),
);

/// One chunk on the GPU, and what it was made from.
struct GpuChunk {
    look: ChunkLook,
    /// `None` for a chunk with no triangles at any level.
    group: Option<wgpu::BindGroup>,
    /// Each level's corners, as the draw's vertex range.
    lods: [Range<u32>; 3],
}

/// What every chunk shares: the materials, the texels of those in use, and the
/// spots, bound with the level's frame.
struct Shared {
    bind_group: wgpu::BindGroup,
    /// The buffers bound beside a frame, for a vista layer to bind with its own.
    buffers: [wgpu::Buffer; 4],
    /// By material id: whether its texels are in the buffer.
    held: [bool; MATERIALS],
}

/// A chunk, its corners at the level in view, and the lights that reach it.
pub(crate) type VoxelDraw = (usize, Range<u32>, u32);

pub(crate) struct VoxelPass {
    uniform: wgpu::Buffer,
    frame_uniform: wgpu::Buffer,
    finish: Finish,
    /// Both taken with the first chunk, as most levels have no voxels, and a new
    /// scene's first frame is what an edit costs in the editor. The texels are
    /// only those of materials in use, made again when a sculpt brings another.
    pipeline: Option<wgpu::RenderPipeline>,
    shared: Option<Shared>,
    /// By material id: whether a chunk on the GPU, or one that failed on its way
    /// there, uses it. Kept apart from `shared`, so that a sync that fails half
    /// way still asks for the materials of the chunks it did put there.
    wanted: [bool; MATERIALS],
    chunks: Vec<Option<GpuChunk>>,
    list: VoxelList,
    draws: Vec<VoxelDraw>,
}

fn push_vec3(out: &mut Vec<u8>, v: mscent_math::Vec3, w: [u8; 4]) {
    for value in [v.x, v.y, v.z] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&w);
}

/// Appends one level's vertices and corners. A triangle naming a vertex the
/// mesh does not have is left out.
fn push_lod(vertices: &mut Vec<u8>, corners: &mut Vec<u8>, lod: &LodLook, base: u32) -> u32 {
    let count = lod.mesh.vertices.len();
    for (i, v) in lod.mesh.vertices.iter().enumerate() {
        let light = lod.light.get(i).copied().unwrap_or_default();
        let tangent = lod.tangent_light.get(i).copied().unwrap_or_default();
        let axis = u32::from(lod.axis.get(i).copied().unwrap_or(2));
        let material = u32::from(lod.material.get(i).copied().unwrap_or(0));
        push_vec3(vertices, v.pos, (axis | material << 8).to_le_bytes());
        push_vec3(vertices, v.normal, tangent.x.to_le_bytes());
        push_vec3(vertices, light.color, tangent.y.to_le_bytes());
        push_vec3(vertices, light.dir, tangent.z.to_le_bytes());
    }
    let mut written = 0;
    for t in &lod.mesh.triangles {
        if t.iter().any(|&i| usize::from(i) >= count) {
            continue;
        }
        for &i in t {
            corners.extend_from_slice(&(base + u32::from(i)).to_le_bytes());
        }
        written += 3;
    }
    written
}

fn pipeline(gpu: &Gpu, finish: Finish) -> wgpu::RenderPipeline {
    let device = &gpu.device;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("voxels"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let spots = spot_field();
    let constants = [
        ("SEAM_BAND", f64::from(SEAM_BAND)),
        ("IN_SEAM", f64::from(IN_SEAM)),
        ("TEXELS_PER_UNIT", f64::from(TEXELS_PER_UNIT)),
        ("MATERIAL_BAND", f64::from(MATERIAL_BAND)),
        ("SHARP_RATE", f64::from(SHARP_RATE)),
        ("LONG_AXIS_LIMIT", f64::from(LONG_AXIS_LIMIT)),
        ("BUMP_FULL", f64::from(BUMP_FULL)),
        ("BUMP_GONE", f64::from(BUMP_GONE)),
        ("SPEC_GAIN", f64::from(SPEC_GAIN)),
        ("MIN_SPEC", f64::from(MIN_SPEC)),
        ("SPOT_EDGE", f64::from(spots.edge)),
        ("SPOT_SPACING", f64::from(spots.spacing)),
        ("SPOT_BAND", f64::from(spots.band)),
        ("SPOT_MIN_EDGE", f64::from(spots.min_edge)),
        ("SPOT_LOST_AT", f64::from(spots.lost_at)),
        ("SPOT_MEAN", f64::from(spots.mean)),
        ("SPOT_COLOR", f64::from(spots.color)),
        ("MIN_FLAT", f64::from(MIN_FLAT)),
        ("MIN_GAIN", f64::from(MIN_GAIN)),
        ("MAX_GAIN", f64::from(MAX_GAIN)),
        ("RELIEF", f64::from(RELIEF)),
        ("KNEE", f64::from(KNEE)),
        ("ROLL", f64::from(ROLL)),
        ("TABLE_LEN", TABLE_LEN as f64),
    ];
    let options = wgpu::PipelineCompilationOptions {
        constants: &with_common(&constants),
        ..Default::default()
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("voxels"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: None,
            compilation_options: options.clone(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: None,
            compilation_options: options,
            targets: &[Some(COLOR.into())],
        }),
        // Triangles wind counter-clockwise seen from the air, and software drops
        // the others before it fills.
        primitive: wgpu::PrimitiveState {
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Greater),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: finish.multisample(),
        multiview_mask: None,
        cache: None,
    })
}

/// What the shader reads about this frame beyond `Frame`.
pub(crate) fn uniform_bytes(cam: &Camera) -> Vec<u8> {
    let mut out = Vec::with_capacity(UNIFORM_BYTES as usize);
    let eye = eye_of(cam);
    let size = [cam.width() as f32, cam.height() as f32];
    for value in [
        eye.x,
        eye.y,
        eye.z,
        cam.focal(),
        size[0],
        size[1],
        cam.near(),
        0.0,
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for (u, v) in AXES {
        push_vec3(&mut out, u, [0; 4]);
        push_vec3(&mut out, v, [0; 4]);
    }
    for row in BAYER {
        for value in row {
            out.extend_from_slice(&u32::from(value).to_le_bytes());
        }
    }
    out
}

/// The shared bind group of `pipeline`, with a view's `frame` and `uniform`.
fn bind(
    gpu: &Gpu,
    pipeline: &wgpu::RenderPipeline,
    (frame, uniform): (&wgpu::Buffer, &wgpu::Buffer),
    buffers: &[wgpu::Buffer; 4],
) -> wgpu::BindGroup {
    let [table, texels, tables, spots] = buffers;
    let entries = [
        (0, frame.as_entire_binding()),
        (1, uniform.as_entire_binding()),
        (2, table.as_entire_binding()),
        (3, texels.as_entire_binding()),
        (4, tables.as_entire_binding()),
        (5, spots.as_entire_binding()),
    ]
    .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("voxels"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    })
}

impl VoxelPass {
    pub(crate) fn new(gpu: &Gpu, frame_uniform: &wgpu::Buffer, finish: Finish) -> Self {
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("voxels"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            uniform,
            frame_uniform: frame_uniform.clone(),
            finish,
            pipeline: None,
            shared: None,
            wanted: [false; MATERIALS],
            chunks: Vec::new(),
            list: VoxelList::unprojected(),
            draws: Vec::new(),
        }
    }

    /// Where the frame's walk leaves the chunks of the leaves in view.
    pub(crate) fn candidates(&mut self) -> &mut Vec<u32> {
        &mut self.list.candidates
    }

    fn make_shared(
        &self,
        gpu: &Gpu,
        assets: &LevelAssets,
        pipeline: &wgpu::RenderPipeline,
        held: [bool; MATERIALS],
    ) -> Result<Shared, GpuError> {
        let built = voxel_materials::build(assets, &held)?;
        let field = spot_field();
        let spot_bytes: Vec<u8> = field
            .distance
            .iter()
            .flat_map(|&d| i32::from(d).to_le_bytes())
            .collect();
        let table = storage(gpu, "voxel materials", &built.table, 4)?;
        let texels = storage(gpu, "voxel texels", &built.texels, 4)?;
        let tables = storage(gpu, "voxel spec", &built.tables, 4 * TABLE_LEN as u64)?;
        let spots = storage(gpu, "spots", &spot_bytes, 4)?;
        let buffers = [table, texels, tables, spots];
        let bind_group = bind(
            gpu,
            pipeline,
            (&self.frame_uniform, &self.uniform),
            &buffers,
        );
        Ok(Shared {
            bind_group,
            buffers,
            held,
        })
    }

    /// The shared bind group for a view with `frame` and `uniform` of its own,
    /// once a chunk has made the pipeline. A sync can remake what it binds, so
    /// it is made for the frame that asks.
    pub(crate) fn bind_view(
        &self,
        gpu: &Gpu,
        frame: &wgpu::Buffer,
        uniform: &wgpu::Buffer,
    ) -> Option<wgpu::BindGroup> {
        let (pipeline, shared) = (self.pipeline.as_ref()?, self.shared.as_ref()?);
        Some(bind(gpu, pipeline, (frame, uniform), &shared.buffers))
    }

    /// The draws of the chunks in `list`, at the level it chose for each.
    pub(crate) fn draws_of(&self, list: &VoxelList, out: &mut Vec<VoxelDraw>) {
        for listed in list.chunks() {
            let c = listed.chunk as usize;
            let Some(chunk) = self.chunks.get(c).and_then(Option::as_ref) else {
                continue;
            };
            let range = chunk.lods[usize::from(listed.lod.min(2))].clone();
            if chunk.group.is_some() && !range.is_empty() {
                out.push((c, range, u32::from(listed.lights)));
            }
        }
    }

    fn upload(
        gpu: &Gpu,
        pipeline: &wgpu::RenderPipeline,
        look: ChunkLook,
    ) -> Result<GpuChunk, GpuError> {
        let (mut vertices, mut corners) = (Vec::new(), Vec::new());
        let mut lods: [Range<u32>; 3] = Default::default();
        let (mut base, mut end) = (0u32, 0u32);
        for (level, range) in lods.iter_mut().enumerate() {
            let lod = look.lod(level);
            // Checked first, so that no corner `push_lod` writes can overflow.
            let count = u32::try_from(lod.mesh.vertices.len()).ok();
            let next_base = count
                .and_then(|n| base.checked_add(n))
                .ok_or(GpuError::TooLarge("a voxel chunk"))?;
            let written = push_lod(&mut vertices, &mut corners, &lod, base);
            base = next_base;
            let next = end
                .checked_add(written)
                .ok_or(GpuError::TooLarge("a voxel chunk"))?;
            *range = end..next;
            end = next;
        }
        if corners.is_empty() {
            return Ok(GpuChunk {
                look,
                group: None,
                lods,
            });
        }
        let vertices = storage(gpu, "voxel vertices", &vertices, VERTEX_BYTES)?;
        let corners = storage(gpu, "voxel corners", &corners, 4)?;
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("voxel chunk"),
            layout: &pipeline.get_bind_group_layout(1),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: vertices.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: corners.as_entire_binding(),
                },
            ],
        });
        Ok(GpuChunk {
            look,
            group: Some(group),
            lods,
        })
    }

    /// Puts on the GPU every chunk that has changed since the last frame, which
    /// is all of them the first time.
    pub(crate) fn sync(&mut self, gpu: &Gpu, assets: &LevelAssets) -> Result<(), GpuError> {
        let voxels: &VoxelScene = assets.scene().voxels();
        let count = voxels.world().chunks.len();
        self.chunks.truncate(count);
        self.chunks.resize_with(count, || None);
        if count == 0 {
            return Ok(());
        }
        let finish = self.finish;
        let compiled = &gpu.voxel_pipelines[finish.index()];
        let pipeline = self
            .pipeline
            .get_or_insert_with(|| compiled.get_or_init(|| pipeline(gpu, finish)).clone());
        for (c, slot) in self.chunks.iter_mut().enumerate() {
            if slot.as_ref().is_some_and(|g| voxels.chunk_is(c, &g.look)) {
                continue;
            }
            let Some(look) = voxels.chunk(c) else {
                continue;
            };
            for level in 0..3 {
                for &m in look.lod(level).material {
                    self.wanted[usize::from(m)] = true;
                }
            }
            *slot = Some(Self::upload(gpu, pipeline, look)?);
        }
        if self.shared.as_ref().map(|s| s.held) != Some(self.wanted) {
            let pipeline = pipeline.clone();
            self.shared = Some(self.make_shared(gpu, assets, &pipeline, self.wanted)?);
        }
        Ok(())
    }

    /// Brings the chunks up to date and lists those in view. An error leaves the
    /// voxels out of this frame, and says why.
    pub(crate) fn prepare(
        &mut self,
        gpu: &Gpu,
        assets: &LevelAssets,
        cam: &Camera,
        lights: &[DynLight],
        stats: &mut SceneStats,
    ) -> Result<(), GpuError> {
        self.draws.clear();
        self.sync(gpu, assets)?;
        let reaching = |leaf| dyn_lights::mask(lights, |l| l.reaches_leaf(leaf)) as u8;
        self.list
            .prepare(assets.scene().voxels(), cam, eye_of(cam), reaching);
        stats.voxel_triangles = self.list.triangles;
        let mut draws = std::mem::take(&mut self.draws);
        self.draws_of(&self.list, &mut draws);
        self.draws = draws;
        if !self.draws.is_empty() {
            gpu.queue
                .write_buffer(&self.uniform, 0, &uniform_bytes(cam));
        }
        Ok(())
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass) {
        if let Some(shared) = &self.shared {
            self.draw_with(pass, &shared.bind_group, &self.draws);
        }
    }

    /// `draws`, with `group` as the shared bind group.
    pub(crate) fn draw_with(
        &self,
        pass: &mut wgpu::RenderPass,
        group: &wgpu::BindGroup,
        draws: &[VoxelDraw],
    ) {
        let Some(pipeline) = &self.pipeline else {
            return;
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        for (c, range, lights) in draws {
            let group = self.chunks.get(*c).and_then(Option::as_ref);
            if let Some(group) = group.and_then(|g| g.group.as_ref()) {
                pass.set_bind_group(1, group, &[]);
                // The instance is the chunk's light mask, for its corners.
                pass.draw(range.clone(), *lights..lights + 1);
            }
        }
    }
}

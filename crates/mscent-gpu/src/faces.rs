//! The level's faces and movers: one vertex buffer for all of them, and each
//! frame an index list of those in view, grouped into one draw per material.

use std::ops::Range;

use mscent_math::Vec3;
use mscent_math::epsilon::MIN_LENGTH_F32;
use mscent_render::bump::{MAX_GAIN, MIN_FLAT, MIN_GAIN, RELIEF};
use mscent_render::clip::MAX_POLYGON_VERTICES;
use mscent_render::light_sampler::MIN_BLEND;
use mscent_render::lighting_shader::{BUMP_FULL, BUMP_GONE};
use mscent_render::raster::{LONG_AXIS_LIMIT, SHARP_RATE};
use mscent_render::scene::{Scene, eye_of};
use mscent_render::texel_light::{KNEE, MIN_SPEC, ROLL, SPEC_GAIN};
use mscent_render::{Camera, DynLight, LevelAssets, MoverPose, Vertex};

use crate::common::with_common;
use crate::device::{Gpu, GpuError};
use crate::dyn_lights::FaceMasks;
use crate::face_data::{self, TABLE_LEN};
use crate::finish::Finish;
use crate::lightmap::LightAtlas;
use crate::targets::{COLOR, DEPTH};
use crate::textures;

/// Position, texel coordinates, face.
const VERTEX_BYTES: usize = 24;
const INDEX_BYTES: usize = 4;
/// A `Pose` of the shader: a matrix and the eye.
const POSE_BYTES: usize = 80;
const SHADER: &str = concat!(
    include_str!("../shaders/common.wgsl"),
    include_str!("../shaders/shading.wgsl"),
    include_str!("../shaders/face.wgsl"),
);

/// A run of the index list, with one material and one pose.
struct Draw {
    material: usize,
    pose: u32,
    indices: Range<u32>,
}

pub(crate) struct FacePass {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    material_groups: Vec<wgpu::BindGroup>,
    /// By face: which of `material_groups` draws it.
    material_of: Vec<usize>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    poses: wgpu::Buffer,
    lights: FaceMasks,
    /// By face: its vertices, empty for one the rasterizer would refuse.
    corners: Vec<Range<u32>>,
    /// By material, this frame's visible faces, kept so a frame does not
    /// allocate them again.
    by_material: Vec<Vec<u32>>,
    listed: Vec<u8>,
    pose_bytes: Vec<u8>,
    draws: Vec<Draw>,
    pub(crate) atlas_size: (u32, u32),
}

/// The rasterizer refuses these, so they are not drawn here either.
fn drawable(poly: &[Vertex]) -> bool {
    let finite = poly.iter().all(|v| v.pos.is_finite());
    (3..=MAX_POLYGON_VERTICES).contains(&poly.len()) && finite
}

/// All faces' vertices, and which belong to whom.
fn vertex_bytes(scene: &Scene) -> Result<(Vec<u8>, Vec<Range<u32>>), GpuError> {
    let mut bytes = Vec::new();
    let mut corners = Vec::with_capacity(scene.polys().len());
    let mut next = 0u32;
    for (index, (poly, face)) in scene.polys().iter().zip(scene.faces()).enumerate() {
        if !drawable(poly) {
            corners.push(next..next);
            continue;
        }
        let texture = scene.texture_of(face.material);
        let (tw, th) = (texture.width() as f32, texture.height() as f32);
        let face_index = u32::try_from(index).map_err(|_| GpuError::TooLarge("faces"))?;
        for v in poly {
            for value in [v.pos.x, v.pos.y, v.pos.z, v.uv.x * tw, v.uv.y * th] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&face_index.to_le_bytes());
        }
        let end = u32::try_from(poly.len())
            .ok()
            .and_then(|count| next.checked_add(count))
            .ok_or(GpuError::TooLarge("vertices"))?;
        corners.push(next..end);
        next = end;
    }
    Ok((bytes, corners))
}

/// The most index bytes a frame can list: every face in view.
fn most_index_bytes(corners: &[Range<u32>]) -> Result<u64, GpuError> {
    corners
        .iter()
        .try_fold(0u64, |total, c| {
            let triangles = c.len().saturating_sub(2) as u64;
            total.checked_add(triangles.checked_mul(3 * INDEX_BYTES as u64)?)
        })
        .ok_or(GpuError::TooLarge("indices"))
}

fn buffer(
    gpu: &Gpu,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> Result<wgpu::Buffer, GpuError> {
    if size > gpu.device.limits().max_buffer_size {
        return Err(GpuError::TooLarge("a buffer"));
    }
    Ok(gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    }))
}

/// A storage buffer holding `bytes`, or `at_least` zero bytes where there are
/// none: a level can have no faces, and a binding must hold one element. More
/// than a shader may bind is an error, not a buffer it would read short.
pub(crate) fn storage(
    gpu: &Gpu,
    label: &str,
    bytes: &[u8],
    at_least: u64,
) -> Result<wgpu::Buffer, GpuError> {
    let size = (bytes.len() as u64).max(at_least);
    if size > gpu.device.limits().max_storage_buffer_binding_size {
        return Err(GpuError::TooLarge("a storage buffer"));
    }
    let buffer = buffer(gpu, label, size, wgpu::BufferUsages::STORAGE)?;
    gpu.queue.write_buffer(&buffer, 0, bytes);
    Ok(buffer)
}

fn pipeline(gpu: &Gpu, finish: Finish) -> wgpu::RenderPipeline {
    let device = &gpu.device;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("faces"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let constants = [
        ("SHARP_RATE", f64::from(SHARP_RATE)),
        ("LONG_AXIS_LIMIT", f64::from(LONG_AXIS_LIMIT)),
        ("MIN_BLEND", f64::from(MIN_BLEND)),
        ("BUMP_FULL", f64::from(BUMP_FULL)),
        ("BUMP_GONE", f64::from(BUMP_GONE)),
        ("MIN_FLAT", f64::from(MIN_FLAT)),
        ("MIN_GAIN", f64::from(MIN_GAIN)),
        ("MAX_GAIN", f64::from(MAX_GAIN)),
        ("RELIEF", f64::from(RELIEF)),
        ("KNEE", f64::from(KNEE)),
        ("ROLL", f64::from(ROLL)),
        ("SPEC_GAIN", f64::from(SPEC_GAIN)),
        ("MIN_SPEC", f64::from(MIN_SPEC)),
        ("MIN_LENGTH", f64::from(MIN_LENGTH_F32)),
        ("TABLE_LEN", TABLE_LEN as f64),
        ("FINE", f64::from(u8::from(finish == Finish::Fine))),
    ];
    let options = wgpu::PipelineCompilationOptions {
        constants: &with_common(&constants),
        ..Default::default()
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("faces"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: None,
            compilation_options: options.clone(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VERTEX_BYTES as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x3, 1 => Float32x2, 2 => Uint32
                ],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: None,
            compilation_options: options,
            targets: &[Some(COLOR.into())],
        }),
        // The walk has already dropped faces that point away.
        primitive: Default::default(),
        // Depth is linear in `1/z` as in software: larger is nearer, 0.0 is nothing.
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

/// The bind group of each material in use, and which each face takes. Unknown
/// material ids all share the grey fallback, so a hostile level cannot make a
/// texture per id.
fn materials(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    scene: &Scene,
) -> (Vec<wgpu::BindGroup>, Vec<usize>) {
    let mut seen: Vec<*const mscent_render::Texture> = Vec::new();
    let mut groups = Vec::new();
    let mut material_of = Vec::with_capacity(scene.faces().len());
    for face in scene.faces() {
        let texture = scene.texture_of(face.material);
        let at = seen.iter().position(|&t| std::ptr::eq(t, texture));
        material_of.push(at.unwrap_or_else(|| {
            seen.push(texture);
            groups.push(textures::bind_group(gpu, layout, texture));
            groups.len() - 1
        }));
    }
    (groups, material_of)
}

fn sampler(gpu: &Gpu, address: wgpu::AddressMode, finish: Finish) -> wgpu::Sampler {
    // Software takes one whole level per span; a fine frame blends two, which
    // anisotropic filtering needs.
    let mipmap_filter = match finish {
        Finish::Reference => wgpu::MipmapFilterMode::Nearest,
        Finish::Fine => wgpu::MipmapFilterMode::Linear,
    };
    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: address,
        address_mode_v: address,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter,
        anisotropy_clamp: finish.anisotropy(),
        ..Default::default()
    })
}

/// A pose's matrix and eye, as the shader reads them.
fn push_pose(out: &mut Vec<u8>, pose: MoverPose, eye: Vec3) {
    let origin = pose.to_world(Vec3::ZERO);
    let axis = |v: Vec3| pose.to_world(v) - origin;
    let (x, y, z) = (
        axis(Vec3::new(1.0, 0.0, 0.0)),
        axis(Vec3::new(0.0, 1.0, 0.0)),
        axis(Vec3::new(0.0, 0.0, 1.0)),
    );
    let authored = pose.to_authored(eye);
    let values = [
        x.x, x.y, x.z, 0.0, y.x, y.y, y.z, 0.0, z.x, z.y, z.z, 0.0, origin.x, origin.y, origin.z,
        1.0, authored.x, authored.y, authored.z, 0.0,
    ];
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

impl FacePass {
    pub(crate) fn new(
        gpu: &Gpu,
        assets: &LevelAssets,
        frame_uniform: &wgpu::Buffer,
        finish: Finish,
    ) -> Result<Self, GpuError> {
        let scene = assets.scene();
        let atlas = LightAtlas::new(gpu, assets)?;
        let data = face_data::build(assets, &atlas);
        let faces = storage(gpu, "face data", &data.faces, 144)?;
        let levels = storage(gpu, "light levels", &data.levels, 16)?;
        let tables = storage(gpu, "spec tables", &data.tables, 4 * TABLE_LEN as u64)?;
        let pose_size = (assets.movers().len() as u64 + 1)
            .checked_mul(POSE_BYTES as u64)
            .ok_or(GpuError::TooLarge("movers"))?;
        let poses = buffer(gpu, "poses", pose_size, wgpu::BufferUsages::STORAGE)?;
        let lights = FaceMasks::new(gpu, scene.faces().len())?;
        let (vertex_data, corners) = vertex_bytes(scene)?;
        let vertices = buffer(
            gpu,
            "faces",
            (vertex_data.len() as u64).max(VERTEX_BYTES as u64),
            wgpu::BufferUsages::VERTEX,
        )?;
        gpu.queue.write_buffer(&vertices, 0, &vertex_data);
        let index_bytes = most_index_bytes(&corners)?.max(INDEX_BYTES as u64);
        let indices = buffer(gpu, "faces", index_bytes, wgpu::BufferUsages::INDEX)?;
        let pipeline = pipeline(gpu, finish);
        // The atlas is read at its top level, so only the materials are filtered.
        let (clamped, repeat) = (
            sampler(gpu, wgpu::AddressMode::ClampToEdge, Finish::Reference),
            sampler(gpu, wgpu::AddressMode::Repeat, finish),
        );
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("faces"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: faces.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: levels.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: poses.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: tables.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&clamped),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(&repeat),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: lights.buffer.as_entire_binding(),
                },
            ],
        });
        let (material_groups, material_of) =
            materials(gpu, &pipeline.get_bind_group_layout(1), scene);
        let by_material = vec![Vec::new(); material_groups.len()];
        Ok(Self {
            pipeline,
            bind_group,
            material_groups,
            material_of,
            vertices,
            indices,
            poses,
            lights,
            corners,
            by_material,
            listed: Vec::new(),
            pose_bytes: Vec::new(),
            draws: Vec::new(),
            atlas_size: atlas.size,
        })
    }

    /// Appends the triangles of `face` to the index list.
    fn list_face(&mut self, face: u32) {
        let Some(corners) = self.corners.get(face as usize) else {
            return;
        };
        // A convex polygon, as a fan from its first corner.
        for i in corners.start.saturating_add(1)..corners.end.saturating_sub(1) {
            for index in [corners.start, i, i + 1] {
                self.listed.extend_from_slice(&index.to_le_bytes());
            }
        }
    }

    fn list_so_far(&self) -> u32 {
        (self.listed.len() / INDEX_BYTES) as u32
    }

    /// The faces in `visible`, one draw per material.
    fn list_world(&mut self, visible: &[u32]) {
        for faces in &mut self.by_material {
            faces.clear();
        }
        for &face in visible {
            let material = self.material_of.get(face as usize).copied();
            if let Some(faces) = material.and_then(|m| self.by_material.get_mut(m)) {
                faces.push(face);
            }
        }
        let by_material = std::mem::take(&mut self.by_material);
        for (material, faces) in by_material.iter().enumerate() {
            let start = self.list_so_far();
            for &face in faces {
                self.list_face(face);
            }
            self.draws.push(Draw {
                material,
                pose: 0,
                indices: start..self.list_so_far(),
            });
        }
        self.by_material = by_material;
    }

    /// Every mover's faces that can be seen, each at its entry in `poses` or as
    /// authored: `list_movers` in `level/movers.rs`.
    fn list_movers(&mut self, assets: &LevelAssets, cam: &Camera, poses: &[MoverPose]) {
        let eye = eye_of(cam);
        for (i, mover) in assets.movers().iter().enumerate() {
            let pose = poses.get(i).copied().unwrap_or(MoverPose::AUTHORED);
            push_pose(&mut self.pose_bytes, pose, eye);
            // View is along -Z, so all of it is behind the eye past this.
            if cam.world_to_camera(pose.to_world(mover.centre)).z > mover.radius {
                continue;
            }
            let authored_eye = pose.to_authored(eye).to_f64();
            for index in mover.faces.clone() {
                let Some(face) = assets.scene().faces().get(index) else {
                    continue;
                };
                if face.plane.signed_distance(authored_eye) <= 0.0 {
                    continue;
                }
                let start = self.list_so_far();
                self.list_face(index as u32);
                self.draws.push(Draw {
                    material: self.material_of.get(index).copied().unwrap_or(0),
                    pose: i as u32 + 1,
                    indices: start..self.list_so_far(),
                });
            }
        }
    }

    /// Lists this frame's faces and writes what the shader reads for them.
    pub(crate) fn prepare(
        &mut self,
        gpu: &Gpu,
        assets: &LevelAssets,
        cam: &Camera,
        visible: &[u32],
        poses: &[MoverPose],
        lights: &[DynLight],
    ) {
        self.listed.clear();
        self.draws.clear();
        self.pose_bytes.clear();
        push_pose(&mut self.pose_bytes, MoverPose::AUTHORED, eye_of(cam));
        self.list_movers(assets, cam, poses);
        self.list_world(visible);
        gpu.queue.write_buffer(&self.poses, 0, &self.pose_bytes);
        gpu.queue.write_buffer(&self.indices, 0, &self.listed);
        self.lights.write(gpu, lights);
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        for draw in self.draws.iter().filter(|d| !d.indices.is_empty()) {
            let Some(group) = self.material_groups.get(draw.material) else {
                continue;
            };
            pass.set_bind_group(1, group, &[]);
            pass.draw_indexed(draw.indices.clone(), 0, draw.pose..draw.pose + 1);
        }
    }
}

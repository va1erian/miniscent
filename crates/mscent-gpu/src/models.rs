//! Enemy models, as `mscent-render`'s model path draws them: each listed part
//! through the voxel pipeline, in its own units. Its frame's clip matrix takes
//! it to the world first, and its eye and light direction are in its units, so
//! the texture rides on the body and the shader needs nothing of its own.
//!
//! Light at each vertex is the software list's, worked out on the CPU, so the
//! draws carry no dynamic light mask. A glowing triangle takes a frame with no
//! bump or highlight, and copies of its vertices at full light.

use std::num::NonZeroU64;
use std::ops::Range;

use mscent_render::model::{ListedPart, ModelInstance, ModelList, ModelTable};
use mscent_render::scene::SceneStats;
use mscent_render::{Camera, DynLight, LevelAssets, Quality};

use crate::FRAME_BYTES;
use crate::device::{Gpu, GpuError};
use crate::faces::buffer;
use crate::finish::Finish;
use crate::voxel_materials::{self, MATERIALS};
use crate::voxels::{self, UNIFORM_BYTES, VERTEX_BYTES, push_vec3};

/// Bytes a part takes in the uniform buffer: a frame lit, a frame glowing and the
/// voxel uniform, each in a slot of `stride` bytes.
const SLOTS: u64 = 3;
/// Not in the remap: no glowing copy of that vertex yet.
const UNMAPPED: u32 = u32::MAX;

/// The buffers a frame's parts are written to, kept and grown.
struct Held {
    vertices: wgpu::Buffer,
    corners: wgpu::Buffer,
    uniforms: wgpu::Buffer,
    /// Binds `vertices` and `corners`.
    group: wgpu::BindGroup,
}

/// The texels of the materials models use, made again when a model brings another.
struct Materials {
    buffers: [wgpu::Buffer; 4],
    held: [bool; MATERIALS],
}

pub(crate) struct ModelPass {
    finish: Finish,
    pipeline: Option<wgpu::RenderPipeline>,
    materials: Option<Materials>,
    wanted: [bool; MATERIALS],
    list: ModelList,
    held: Option<Held>,
    /// This frame's bytes, kept for their storage.
    vertices: Vec<u8>,
    corners: Vec<u8>,
    uniforms: Vec<u8>,
    remap: Vec<u32>,
    /// By part: its corners lit, then glowing.
    ranges: Vec<[Range<u32>; 2]>,
    /// One per non-empty range, with its frame bound.
    draws: Vec<(wgpu::BindGroup, Range<u32>)>,
}

/// What every part of a frame shares.
struct Frame<'a> {
    cam: &'a Camera,
    fog: Option<mscent_render::Fog>,
    quality: Quality,
    stride: u64,
}

fn vertex_count(bytes: &[u8]) -> Result<u32, GpuError> {
    u32::try_from(bytes.len() as u64 / VERTEX_BYTES)
        .map_err(|_| GpuError::TooLarge("model vertices"))
}

fn corner_count(bytes: &[u8]) -> Result<u32, GpuError> {
    u32::try_from(bytes.len() / 4).map_err(|_| GpuError::TooLarge("model corners"))
}

impl ModelPass {
    pub(crate) fn new(finish: Finish) -> Self {
        Self {
            finish,
            pipeline: None,
            materials: None,
            wanted: [false; MATERIALS],
            list: ModelList::unprojected(),
            held: None,
            vertices: Vec::new(),
            corners: Vec::new(),
            uniforms: Vec::new(),
            remap: Vec::new(),
            ranges: Vec::new(),
            draws: Vec::new(),
        }
    }

    /// Lists the frame's parts and puts them on the GPU. An error leaves the
    /// models out of this frame, and says why.
    pub(crate) fn prepare(
        &mut self,
        gpu: &Gpu,
        assets: &LevelAssets,
        (cam, quality): (&Camera, Quality),
        (models, lights): (Option<(&ModelTable, &[ModelInstance])>, &[DynLight]),
        stats: &mut SceneStats,
    ) -> Result<(), GpuError> {
        self.draws.clear();
        assets.list_models(&mut self.list, models, cam, lights);
        stats.model_triangles = self.list.triangles;
        let Some((table, _)) = models else {
            return Ok(());
        };
        if self.list.parts().is_empty() {
            return Ok(());
        }
        let align = u64::from(gpu.device.limits().min_uniform_buffer_offset_alignment);
        let stride = FRAME_BYTES
            .max(UNIFORM_BYTES)
            .next_multiple_of(align.max(1));
        let frame = Frame {
            cam,
            fog: assets.fog(),
            quality,
            stride,
        };
        self.write_parts(table, &frame)?;
        let finish = self.finish;
        let compiled = &gpu.voxel_pipelines[finish.index()];
        let pipeline = self
            .pipeline
            .get_or_insert_with(|| {
                compiled
                    .get_or_init(|| voxels::pipeline(gpu, finish))
                    .clone()
            })
            .clone();
        self.sync_materials(gpu, assets)?;
        self.upload(gpu, &pipeline)?;
        self.bind_draws(gpu, &pipeline, stride);
        Ok(())
    }

    fn write_parts(&mut self, table: &ModelTable, frame: &Frame) -> Result<(), GpuError> {
        self.vertices.clear();
        self.corners.clear();
        self.uniforms.clear();
        self.ranges.clear();
        let parts = std::mem::take(&mut self.list);
        let result = parts.parts().iter().try_for_each(|listed| {
            let ranges = match table.level_of(listed) {
                Some(level) => self.write_part(&parts, listed, level)?,
                None => [0..0, 0..0],
            };
            self.ranges.push(ranges);
            self.write_uniforms(listed, frame);
            Ok(())
        });
        self.list = parts;
        result
    }

    /// One part's vertices lit, its corners, then copies of the vertices of its
    /// glowing triangles at full light and their corners.
    fn write_part(
        &mut self,
        list: &ModelList,
        listed: &ListedPart,
        level: &mscent_render::model::Level,
    ) -> Result<[Range<u32>; 2], GpuError> {
        let base = vertex_count(&self.vertices)?;
        let count = level.vertices.len();
        for i in 0..count {
            let v = list.vertex(listed, level, i, false);
            self.push_vertex(v.ok_or(GpuError::TooLarge("a model level"))?, listed);
        }
        vertex_count(&self.vertices)?;
        let lit_start = corner_count(&self.corners)?;
        let triangles = level.triangles.iter().zip(&level.glow);
        for (t, _) in triangles.clone().filter(|(_, glow)| !**glow) {
            if t.iter().all(|&i| usize::from(i) < count) {
                for &i in t {
                    self.corners
                        .extend_from_slice(&(base + u32::from(i)).to_le_bytes());
                }
            }
        }
        let glow_start = corner_count(&self.corners)?;
        self.remap.clear();
        self.remap.resize(count, UNMAPPED);
        for (t, _) in triangles.filter(|(_, glow)| **glow) {
            if t.iter().any(|&i| usize::from(i) >= count) {
                continue;
            }
            for &i in t {
                let i = usize::from(i);
                if self.remap[i] == UNMAPPED {
                    self.remap[i] = vertex_count(&self.vertices)?;
                    let v = list.vertex(listed, level, i, true);
                    self.push_vertex(v.ok_or(GpuError::TooLarge("a model level"))?, listed);
                }
                self.corners.extend_from_slice(&self.remap[i].to_le_bytes());
            }
        }
        let end = corner_count(&self.corners)?;
        Ok([lit_start..glow_start, glow_start..end])
    }

    /// A `Vertex` of `voxel.wgsl`.
    fn push_vertex(&mut self, v: mscent_render::model::ModelVertex, listed: &ListedPart) {
        self.wanted[usize::from(v.material)] = true;
        let look = u32::from(v.axis) | u32::from(v.material) << 8;
        let out = &mut self.vertices;
        push_vec3(out, v.pos, look.to_le_bytes());
        push_vec3(out, v.normal, v.tangent_dir.x.to_le_bytes());
        push_vec3(out, v.light, v.tangent_dir.y.to_le_bytes());
        push_vec3(out, listed.dir, v.tangent_dir.z.to_le_bytes());
    }

    /// The part's frame lit, its frame glowing, and its voxel uniform, each
    /// padded to a slot.
    fn write_uniforms(&mut self, listed: &ListedPart, frame: &Frame) {
        let clip = frame.cam.gpu_clip_matrix() * listed.to_world();
        // Low has neither bump nor highlight, as a glowing triangle has neither.
        for quality in [frame.quality, Quality::Low] {
            let bytes = crate::frame_bytes(clip, (frame.fog, &[]), quality, (0, 0));
            self.push_slot(&bytes, frame.stride);
        }
        let bytes = voxels::uniform_bytes_at(frame.cam, listed.eye);
        self.push_slot(&bytes, frame.stride);
    }

    fn push_slot(&mut self, bytes: &[u8], stride: u64) {
        let start = self.uniforms.len();
        self.uniforms.extend_from_slice(bytes);
        self.uniforms.resize(start + stride as usize, 0);
    }

    fn sync_materials(&mut self, gpu: &Gpu, assets: &LevelAssets) -> Result<(), GpuError> {
        let held = self.materials.as_ref().map(|m| m.held);
        let covered = held.is_some_and(|h| h.iter().zip(&self.wanted).all(|(h, w)| *h || !*w));
        if covered {
            return Ok(());
        }
        let built = voxel_materials::build(assets, &self.wanted)?;
        self.materials = Some(Materials {
            buffers: voxels::material_buffers(gpu, &built)?,
            held: self.wanted,
        });
        Ok(())
    }

    /// Writes the frame's bytes, growing a buffer that is too small.
    fn upload(&mut self, gpu: &Gpu, pipeline: &wgpu::RenderPipeline) -> Result<(), GpuError> {
        let fits = |b: &wgpu::Buffer, bytes: &[u8]| b.size() >= bytes.len().max(4) as u64;
        let held = self.held.as_ref().filter(|h| {
            fits(&h.vertices, &self.vertices)
                && fits(&h.corners, &self.corners)
                && fits(&h.uniforms, &self.uniforms)
        });
        if held.is_none() {
            let size = |bytes: &[u8]| (bytes.len() as u64).max(4).next_power_of_two();
            let storage_of =
                |label, bytes: &[u8]| buffer(gpu, label, size(bytes), wgpu::BufferUsages::STORAGE);
            let vertices = storage_of("model vertices", &self.vertices)?;
            let corners = storage_of("model corners", &self.corners)?;
            let uniforms = buffer(
                gpu,
                "model frames",
                size(&self.uniforms),
                wgpu::BufferUsages::UNIFORM,
            )?;
            let group = voxels::bind_mesh(gpu, pipeline, &vertices, &corners);
            self.held = Some(Held {
                vertices,
                corners,
                uniforms,
                group,
            });
        }
        let Some(held) = &self.held else {
            return Ok(());
        };
        gpu.queue.write_buffer(&held.vertices, 0, &self.vertices);
        gpu.queue.write_buffer(&held.corners, 0, &self.corners);
        gpu.queue.write_buffer(&held.uniforms, 0, &self.uniforms);
        Ok(())
    }

    fn bind_draws(&mut self, gpu: &Gpu, pipeline: &wgpu::RenderPipeline, stride: u64) {
        let (Some(held), Some(materials)) = (&self.held, &self.materials) else {
            return;
        };
        let layout = pipeline.get_bind_group_layout(0);
        let slot = |at: u64, size: u64| {
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &held.uniforms,
                offset: at,
                size: NonZeroU64::new(size),
            })
        };
        let [table, texels, tables, spots] = &materials.buffers;
        for (p, ranges) in (0u64..).zip(&self.ranges) {
            let base = p * SLOTS * stride;
            for (k, range) in (0u64..).zip(ranges) {
                if range.is_empty() {
                    continue;
                }
                let entries = [
                    (0, slot(base + k * stride, FRAME_BYTES)),
                    (1, slot(base + 2 * stride, UNIFORM_BYTES)),
                    (2, table.as_entire_binding()),
                    (3, texels.as_entire_binding()),
                    (4, tables.as_entire_binding()),
                    (5, spots.as_entire_binding()),
                ]
                .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
                let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("model part"),
                    layout: &layout,
                    entries: &entries,
                });
                self.draws.push((group, range.clone()));
            }
        }
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass) {
        let (Some(pipeline), Some(held)) = (&self.pipeline, &self.held) else {
            return;
        };
        if self.draws.is_empty() {
            return;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(1, &held.group, &[]);
        for (group, range) in &self.draws {
            pass.set_bind_group(0, group, &[]);
            // Instance 0: no dynamic light mask, as the vertices carry their light.
            pass.draw(range.clone(), 0..1);
        }
    }
}

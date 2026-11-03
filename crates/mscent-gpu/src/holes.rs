//! Viewport faces: walls that show a vista. Each frame lists those the walk
//! found, and they draw last of the level's solid things, at their true depth,
//! each with its slot's picture.

use std::ops::Range;

use mscent_render::Vertex;
use mscent_render::clip::MAX_POLYGON_VERTICES;
use mscent_render::scene::{HoleList, Scene};
use mscent_world::materials::VIEW_SLOTS;

use crate::common::with_common;
use crate::device::{Gpu, GpuError};
use crate::faces::buffer;
use crate::finish::Finish;
use crate::targets::{COLOR, DEPTH};

/// Position and the layer of the vista picture.
const VERTEX_BYTES: u64 = 16;
const INDEX_BYTES: u64 = 4;
const SHADER: &str = concat!(
    include_str!("../shaders/common.wgsl"),
    include_str!("../shaders/hole.wgsl"),
);

pub(crate) struct Holes {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// By viewport face: its vertices, empty for one the rasterizer would refuse.
    corners: Vec<Range<u32>>,
    /// By slot: its layer of the vista pictures, for a slot some face shows.
    layer_of: [Option<u32>; VIEW_SLOTS as usize],
    listed: Vec<u8>,
}

/// The rasterizer refuses these, so they are not drawn here either.
fn drawable(poly: &[Vertex]) -> bool {
    let finite = poly.iter().all(|v| v.pos.is_finite());
    (3..=MAX_POLYGON_VERTICES).contains(&poly.len()) && finite
}

/// By slot, its layer of the pictures: the slots the level uses, in order.
fn layers_of(slots: &[u8]) -> [Option<u32>; VIEW_SLOTS as usize] {
    let mut layer_of = [None; VIEW_SLOTS as usize];
    let mut next = 0;
    for slot in 0..VIEW_SLOTS {
        if slots.contains(&slot) {
            layer_of[usize::from(slot)] = Some(next);
            next += 1;
        }
    }
    layer_of
}

fn vertex_bytes(
    scene: &Scene,
    layer_of: &[Option<u32>],
) -> Result<(Vec<u8>, Vec<Range<u32>>), GpuError> {
    let (mut bytes, mut corners, mut next) = (Vec::new(), Vec::new(), 0u32);
    for (poly, &slot) in scene.viewport_polys().iter().zip(scene.viewport_slots()) {
        let layer = layer_of.get(usize::from(slot)).copied().flatten();
        let Some(layer) = layer.filter(|_| drawable(poly)) else {
            corners.push(next..next);
            continue;
        };
        for v in poly {
            for value in [v.pos.x, v.pos.y, v.pos.z] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&layer.to_le_bytes());
        }
        let end = u32::try_from(poly.len())
            .ok()
            .and_then(|count| next.checked_add(count))
            .ok_or(GpuError::TooLarge("viewport faces"))?;
        corners.push(next..end);
        next = end;
    }
    Ok((bytes, corners))
}

fn pipeline(gpu: &Gpu, finish: Finish) -> wgpu::RenderPipeline {
    let device = &gpu.device;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("viewport faces"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let options = wgpu::PipelineCompilationOptions {
        constants: &with_common(&[]),
        ..Default::default()
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("viewport faces"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: None,
            compilation_options: options.clone(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VERTEX_BYTES,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: None,
            compilation_options: options,
            targets: &[Some(COLOR.into())],
        }),
        // The walk lists only the faces that face the eye.
        primitive: Default::default(),
        // True depth, so that what the flood listed beyond the wall stays hidden.
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

impl Holes {
    pub(crate) fn new(gpu: &Gpu, scene: &Scene, finish: Finish) -> Result<Self, GpuError> {
        let layer_of = layers_of(scene.viewport_slots());
        let (vertex_data, corners) = vertex_bytes(scene, &layer_of)?;
        let vertices = buffer(
            gpu,
            "viewport faces",
            (vertex_data.len() as u64).max(VERTEX_BYTES),
            wgpu::BufferUsages::VERTEX,
        )?;
        gpu.queue.write_buffer(&vertices, 0, &vertex_data);
        let mut triangles = corners.iter().map(|c| c.len().saturating_sub(2) as u64);
        let index_bytes = triangles
            .try_fold(0u64, |sum, t| {
                sum.checked_add(t.checked_mul(3 * INDEX_BYTES)?)
            })
            .ok_or(GpuError::TooLarge("viewport faces"))?;
        let indices = buffer(
            gpu,
            "viewport faces",
            index_bytes.max(INDEX_BYTES),
            wgpu::BufferUsages::INDEX,
        )?;
        Ok(Self {
            pipeline: (gpu.hole_pipelines[finish.index()].get_or_init(|| pipeline(gpu, finish)))
                .clone(),
            vertices,
            indices,
            corners,
            layer_of,
            listed: Vec::new(),
        })
    }

    /// How many pictures the level's slots need.
    pub(crate) fn layer_count(&self) -> u32 {
        self.layer_of.iter().flatten().count() as u32
    }

    /// The layer of the pictures that `slot` draws into.
    pub(crate) fn layer_of(&self, slot: u8) -> Option<u32> {
        self.layer_of.get(usize::from(slot)).copied().flatten()
    }

    pub(crate) fn layout(&self) -> wgpu::BindGroupLayout {
        self.pipeline.get_bind_group_layout(0)
    }

    /// Lists the faces in `holes`, each once, as the walk found them.
    pub(crate) fn prepare(&mut self, gpu: &Gpu, holes: &HoleList) {
        self.listed.clear();
        for &id in holes.ids() {
            let Some(corners) = self.corners.get(id as usize) else {
                continue;
            };
            for i in corners.start.saturating_add(1)..corners.end.saturating_sub(1) {
                for index in [corners.start, i, i + 1] {
                    self.listed.extend_from_slice(&index.to_le_bytes());
                }
            }
        }
        if !self.listed.is_empty() {
            gpu.queue.write_buffer(&self.indices, 0, &self.listed);
        }
    }

    /// Draws the listed faces with `pictures`, bound with [`Self::layout`].
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass, pictures: &wgpu::BindGroup) {
        let count = (self.listed.len() as u64 / INDEX_BYTES) as u32;
        if count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, pictures, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..count, 0, 0..1);
    }
}

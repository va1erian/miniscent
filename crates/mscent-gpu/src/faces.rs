//! The level's faces: one vertex buffer for all of them, and each frame an
//! index list of those in view. Each face is one flat colour for now, the
//! texture's mean under [`orientation_light`].

use std::ops::Range;

use mscent_render::clip::MAX_POLYGON_VERTICES;
use mscent_render::scene::{Scene, orientation_light};
use mscent_render::shade::mul_rgb;
use mscent_render::{Camera, Vertex};

use crate::device::{Gpu, GpuError};
use crate::targets::{COLOR, DEPTH};

/// Position, then colour as the bytes of `0xAARRGGBB`.
const VERTEX_BYTES: usize = 16;
const INDEX_BYTES: usize = 4;
const MATRIX_BYTES: u64 = 64;

pub(crate) struct FacePass {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// By face: its vertices, empty for one the rasterizer would refuse.
    corners: Vec<Range<u32>>,
    /// This frame's index list, kept so a frame does not allocate it again.
    listed: Vec<u8>,
}

/// The rasterizer refuses these, so they are not drawn here either.
fn drawable(poly: &[Vertex]) -> bool {
    let finite = poly.iter().all(|v| v.pos.is_finite());
    (3..=MAX_POLYGON_VERTICES).contains(&poly.len()) && finite
}

/// The colour of every vertex of a face.
fn flat_color(scene: &Scene, index: usize) -> u32 {
    let Some(face) = scene.faces().get(index) else {
        return 0xff00_0000;
    };
    let texture = scene.texture_of(face.material);
    // The last mip level is the texture's mean.
    let mean = texture.texel_lod(0, 0, texture.mip_count());
    mul_rgb(mean, orientation_light(face))
}

/// All faces' vertices, and which belong to whom.
fn vertex_bytes(scene: &Scene) -> Result<(Vec<u8>, Vec<Range<u32>>), GpuError> {
    let mut bytes = Vec::new();
    let mut corners = Vec::with_capacity(scene.polys().len());
    let mut next = 0u32;
    for (index, poly) in scene.polys().iter().enumerate() {
        if !drawable(poly) {
            corners.push(next..next);
            continue;
        }
        let color = flat_color(scene, index).to_le_bytes();
        for v in poly {
            for axis in [v.pos.x, v.pos.y, v.pos.z] {
                bytes.extend_from_slice(&axis.to_le_bytes());
            }
            bytes.extend_from_slice(&color);
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
        // A level can have no faces, and a binding cannot be empty.
        size: size.max(INDEX_BYTES as u64),
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    }))
}

fn pipeline(gpu: &Gpu) -> wgpu::RenderPipeline {
    let device = &gpu.device;
    let shader = device.create_shader_module(wgpu::include_wgsl!("../shaders/face.wgsl"));
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("faces"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: None,
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VERTEX_BYTES as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: None,
            compilation_options: Default::default(),
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
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

impl FacePass {
    pub(crate) fn new(gpu: &Gpu, scene: &Scene) -> Result<Self, GpuError> {
        let (vertex_data, corners) = vertex_bytes(scene)?;
        let vertices = buffer(
            gpu,
            "faces",
            vertex_data.len() as u64,
            wgpu::BufferUsages::VERTEX,
        )?;
        gpu.queue.write_buffer(&vertices, 0, &vertex_data);
        let index_bytes = most_index_bytes(&corners)?;
        let indices = buffer(gpu, "faces", index_bytes, wgpu::BufferUsages::INDEX)?;
        let uniform = buffer(gpu, "frame", MATRIX_BYTES, wgpu::BufferUsages::UNIFORM)?;
        let pipeline = pipeline(gpu);
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("faces"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Ok(Self {
            pipeline,
            uniform,
            bind_group,
            vertices,
            indices,
            corners,
            listed: Vec::new(),
        })
    }

    /// Lists the faces in `visible` for this frame's camera.
    pub(crate) fn prepare(&mut self, gpu: &Gpu, cam: &Camera, visible: &[u32]) {
        let matrix = cam.gpu_clip_matrix();
        let floats = matrix.cols.iter().flat_map(|c| [c.x, c.y, c.z, c.w]);
        let bytes: Vec<u8> = floats.flat_map(f32::to_le_bytes).collect();
        gpu.queue.write_buffer(&self.uniform, 0, &bytes);
        self.listed.clear();
        for corners in visible.iter().filter_map(|&f| self.corners.get(f as usize)) {
            // A convex polygon, as a fan from its first corner.
            for i in corners.start.saturating_add(1)..corners.end.saturating_sub(1) {
                for index in [corners.start, i, i + 1] {
                    self.listed.extend_from_slice(&index.to_le_bytes());
                }
            }
        }
        gpu.queue.write_buffer(&self.indices, 0, &self.listed);
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass) {
        let count = (self.listed.len() / INDEX_BYTES) as u32;
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..count, 0, 0..1);
    }
}

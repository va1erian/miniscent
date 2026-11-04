//! Actors: their triangles lit on the CPU each frame, as the software path does,
//! so a frame's lighting has one source.

use mscent_math::Vec3;
use mscent_render::Camera;
use mscent_render::mesh::MAX_LIGHT;
use mscent_render::scene::eye_of;
use mscent_render::{Actor, DynLight, LevelAssets};

use crate::common::with_common;
use crate::device::{Gpu, GpuError};
use crate::finish::Finish;
use crate::targets::{COLOR, DEPTH};

/// Position, colour, light.
pub(crate) const VERTEX_BYTES: u64 = 20;
const SHADER: &str = concat!(
    include_str!("../shaders/common.wgsl"),
    include_str!("../shaders/mesh.wgsl"),
);

pub(crate) struct ActorPass {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    vertices: Option<wgpu::Buffer>,
    listed: Vec<u8>,
}

impl ActorPass {
    pub(crate) fn new(gpu: &Gpu, frame_uniform: &wgpu::Buffer, finish: Finish) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("actors"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let constants = [("MAX_LIGHT", f64::from(MAX_LIGHT))];
        let options = wgpu::PipelineCompilationOptions {
            constants: &with_common(&constants),
            ..Default::default()
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("actors"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: None,
                compilation_options: options.clone(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: VERTEX_BYTES,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Uint32, 2 => Float32],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: None,
                compilation_options: options,
                targets: &[Some(COLOR.into())],
            }),
            // Back faces are already gone: `actor_triangles` drops them.
            primitive: Default::default(),
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
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("actors"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_uniform.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            bind_group,
            vertices: None,
            listed: Vec::new(),
        }
    }

    /// Lights and lists `actors`. An actor list too large for the GPU's buffers
    /// is an error, and draws none of them.
    pub(crate) fn prepare(
        &mut self,
        gpu: &Gpu,
        assets: &LevelAssets,
        cam: &Camera,
        actors: &[Actor],
        lights: &[DynLight],
    ) -> Result<(), GpuError> {
        self.list(assets, cam, actors, lights);
        let needed = self.listed.len() as u64;
        let most = gpu.device.limits().max_buffer_size;
        let vertices = needed / VERTEX_BYTES;
        if needed > most || u32::try_from(vertices).is_err() {
            self.listed.clear();
            return Err(GpuError::TooLarge("the actors"));
        }
        if self.vertices.as_ref().is_none_or(|b| b.size() < needed) && needed > 0 {
            self.vertices = Some(gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("actors"),
                size: needed.saturating_mul(2).min(most),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(vertices) = self.vertices.as_ref().filter(|_| needed > 0) {
            gpu.queue.write_buffer(vertices, 0, &self.listed);
        }
        Ok(())
    }

    fn list(&mut self, assets: &LevelAssets, cam: &Camera, actors: &[Actor], lights: &[DynLight]) {
        self.listed.clear();
        let listed = &mut self.listed;
        assets.actor_triangles(actors, eye_of(cam), lights, |corners, color| {
            for (pos, light) in corners {
                for axis in [pos.x, pos.y, pos.z] {
                    listed.extend_from_slice(&axis.to_le_bytes());
                }
                listed.extend_from_slice(&color.to_le_bytes());
                listed.extend_from_slice(&light.to_le_bytes());
            }
        });
    }

    /// A sphere round the listed triangles: the middle of their box, and its half
    /// diagonal. `None` for none.
    pub(crate) fn bound(&self) -> Option<(Vec3, f32)> {
        let corners = self
            .listed
            .as_chunks::<{ VERTEX_BYTES as usize }>()
            .0
            .iter()
            .map(|v| {
                let axis = |i: usize| f32::from_le_bytes([v[i], v[i + 1], v[i + 2], v[i + 3]]);
                Vec3::new(axis(0), axis(4), axis(8))
            });
        let (lo, hi) = corners.fold(None, |found: Option<(Vec3, Vec3)>, p| {
            let (lo, hi) = found.unwrap_or((p, p));
            Some((lo.min(p), hi.max(p)))
        })?;
        Some(((lo + hi) * 0.5, (hi - lo).length() * 0.5))
    }

    /// The vertex buffer and its count, for the shadow maps to draw from.
    pub(crate) fn casters(&self) -> Option<(&wgpu::Buffer, u32)> {
        let count = u32::try_from(self.listed.len() as u64 / VERTEX_BYTES).ok()?;
        Some((self.vertices.as_ref()?, count))
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass) {
        let Some(vertices) = &self.vertices else {
            return;
        };
        // `prepare` holds the count to this.
        let Ok(count) = u32::try_from(self.listed.len() as u64 / VERTEX_BYTES) else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..count, 0..1);
    }
}

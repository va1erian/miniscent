//! A multisampled frame's depth brought down to one value a pixel, which a copy
//! cannot do: wgpu copies no multisampled texture, and resolves no depth.

use crate::device::Gpu;

/// Where the resolved depth goes: the bytes of an `f32`, as a depth texel's.
pub(crate) const RESOLVED: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

pub(crate) struct DepthResolve {
    pipeline: wgpu::RenderPipeline,
}

impl DepthResolve {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let shader =
            device.create_shader_module(wgpu::include_wgsl!("../shaders/depth_resolve.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("depth resolve"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: None,
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: None,
                compilation_options: Default::default(),
                targets: &[Some(RESOLVED.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline }
    }

    /// What [`Self::run`] reads `depth` through.
    pub(crate) fn bind(&self, gpu: &Gpu, depth: &wgpu::TextureView) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("depth resolve"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(depth),
            }],
        })
    }

    /// Writes the depth `source` reads into `into`, one value a pixel.
    pub(crate) fn run(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::BindGroup,
        into: &wgpu::TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("depth resolve"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: into,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, source, &[]);
        pass.draw(0..3, 0..1);
    }
}

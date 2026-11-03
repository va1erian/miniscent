//! The gradient behind everything.

use mscent_render::BuildConfig;

use crate::device::Gpu;
use crate::finish::Finish;
use crate::targets::{COLOR, DEPTH};

/// `top`, `bottom`, `last_row` and the padding a uniform block rounds up to.
const UNIFORM_BYTES: u64 = 16;

pub(crate) struct SkyPass {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    colors: (u32, u32),
}

impl SkyPass {
    pub(crate) fn new(gpu: &Gpu, cfg: &BuildConfig, finish: Finish) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::include_wgsl!("../shaders/sky.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
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
                targets: &[Some(COLOR.into())],
            }),
            primitive: Default::default(),
            // Behind everything, so it neither tests nor writes depth.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: finish.multisample(),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            uniform,
            bind_group,
            colors: (cfg.sky_top, cfg.sky_bottom),
        }
    }

    /// Sets the gradient for a target `height` rows tall.
    pub(crate) fn prepare(&self, gpu: &Gpu, height: u32) {
        let last_row = height.saturating_sub(1).max(1);
        let fields = [self.colors.0, self.colors.1, last_row, 0];
        let bytes: Vec<u8> = fields.iter().flat_map(|v| v.to_le_bytes()).collect();
        gpu.queue.write_buffer(&self.uniform, 0, &bytes);
    }

    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

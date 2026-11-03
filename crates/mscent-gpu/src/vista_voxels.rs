//! The voxel surfaces of vista layers' rooms: the level's chunks, listed from
//! each layer's eye with the budgets and detail levels software gives them, and
//! drawn with that layer's frame. No dynamic light reaches a layer.

use mscent_render::voxel::VoxelList;
use mscent_render::{LevelAssets, VistaView};

use crate::device::{Gpu, GpuError};
use crate::faces::buffer;
use crate::voxels::{UNIFORM_BYTES, VoxelDraw, VoxelPass, uniform_bytes};

/// One view's draws, with what its shader reads beyond its `Frame`.
struct ViewVoxels {
    uniform: wgpu::Buffer,
    group: Option<wgpu::BindGroup>,
    draws: Vec<VoxelDraw>,
}

#[derive(Default)]
pub(crate) struct LayerVoxels {
    /// By layer of the level, kept between frames: a list's detail levels depend
    /// on the frames before, as software's do.
    lists: Vec<VoxelList>,
    /// By view of this frame.
    views: Vec<ViewVoxels>,
}

impl LayerVoxels {
    /// Lists the voxels of each of `views`, whose `Frame` is `frame_of` its
    /// index, once `voxels` is in sync for this frame. Returns the triangles.
    pub(crate) fn prepare<'f>(
        &mut self,
        gpu: &Gpu,
        assets: &LevelAssets,
        (voxels, views): (&VoxelPass, &[VistaView]),
        frame_of: impl Fn(usize) -> Option<&'f wgpu::Buffer>,
    ) -> Result<u32, GpuError> {
        let triangles = assets.vista_voxels(views, &mut self.lists);
        for (i, view) in views.iter().enumerate() {
            if self.views.len() <= i {
                let uniform = buffer(
                    gpu,
                    "layer voxels",
                    UNIFORM_BYTES,
                    wgpu::BufferUsages::UNIFORM,
                )?;
                self.views.push(ViewVoxels {
                    uniform,
                    group: None,
                    draws: Vec::new(),
                });
            }
            let Some(out) = self.views.get_mut(i) else {
                continue;
            };
            out.draws.clear();
            out.group = None;
            if let Some(list) = self.lists.get(view.layer) {
                voxels.draws_of(list, &mut out.draws);
            }
            if out.draws.is_empty() {
                continue;
            }
            gpu.queue
                .write_buffer(&out.uniform, 0, &uniform_bytes(&view.cam));
            out.group = frame_of(i).and_then(|frame| voxels.bind_view(gpu, frame, &out.uniform));
        }
        Ok(triangles)
    }

    /// The voxels of this frame's `i`th view, before its faces as in software.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass, voxels: &VoxelPass, i: usize) {
        let Some(view) = self.views.get(i) else {
            return;
        };
        if let Some(group) = &view.group {
            voxels.draw_with(pass, group, &view.draws);
        }
    }
}

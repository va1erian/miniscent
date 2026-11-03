//! What the viewport faces show. Before the level's frame, each slot in view
//! draws its vista into a picture of its own: the gradient, then its layers far
//! to near, each with a depth of its own, as `fill_vistas` in `level/vista.rs`.
//! The viewport faces then show those pictures at their own depth (`holes.rs`).
//! Software masks a vista to its faces' pixels; here the faces cut it out, which
//! leaves the same pixels.

use mscent_render::scene::HoleList;
use mscent_render::{LevelAssets, View, VistaView};
use mscent_world::materials::VIEW_SLOTS;

use crate::device::{Gpu, GpuError};
use crate::faces::FacePass;
use crate::finish::Finish;
use crate::holes::Holes;
use crate::sky::SkyPass;
use crate::targets::{COLOR, DEPTH};
use crate::vista_voxels::LayerVoxels;
use crate::voxels::VoxelPass;

/// Each slot's picture, and what draws it.
struct Pictures {
    size: (u32, u32),
    /// By layer of the picture array, a view to draw into.
    layers: Vec<wgpu::TextureView>,
    /// Where a multisampled vista draws before it is resolved into its layer.
    samples: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
    /// The array, for the viewport faces to read.
    bind_group: wgpu::BindGroup,
}

pub(crate) struct VistaPass {
    holes: Holes,
    listed: HoleList,
    views: Vec<VistaView>,
    voxels: LayerVoxels,
    finish: Finish,
    frame_uniform: wgpu::Buffer,
    pictures: Option<Pictures>,
}

fn texture(
    gpu: &Gpu,
    (width, height): (u32, u32),
    layers: u32,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::Texture {
    let read = if samples > 1 {
        wgpu::TextureUsages::empty()
    } else {
        wgpu::TextureUsages::TEXTURE_BINDING
    };
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("vista"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | read,
        view_formats: &[],
    })
}

/// The part of a `size` target that `window` covers, with a pixel to spare on
/// each side, as `(x, y, width, height)`; never empty.
fn scissor(window: [f32; 4], (width, height): (u32, u32)) -> (u32, u32, u32, u32) {
    let side = |lo: f32, hi: f32, most: u32| {
        let start = (lo.floor() - 1.0).clamp(0.0, (most - 1) as f32) as u32;
        let end = (hi.ceil() + 1.0).clamp((start + 1) as f32, most as f32) as u32;
        (start, end - start)
    };
    let (x, w) = side(window[0], window[2], width);
    let (y, h) = side(window[1], window[3], height);
    (x, y, w, h)
}

impl Pictures {
    fn new(gpu: &Gpu, size: (u32, u32), count: u32, finish: Finish, pass: &VistaPass) -> Self {
        let count = count.max(1);
        let array = texture(gpu, size, count, COLOR, 1);
        let layer = |l: u32| {
            array.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: l,
                array_layer_count: Some(1),
                ..Default::default()
            })
        };
        let samples = finish.samples();
        let multisampled = (samples > 1).then(|| texture(gpu, size, 1, COLOR, samples));
        let whole = array.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vista pictures"),
            layout: &pass.holes.layout(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: pass.frame_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&whole),
                },
            ],
        });
        Self {
            size,
            layers: (0..count).map(layer).collect(),
            samples: multisampled.map(|t| t.create_view(&Default::default())),
            depth: texture(gpu, size, 1, DEPTH, samples).create_view(&Default::default()),
            bind_group,
        }
    }

    /// A pass drawing one layer of a vista into picture `layer`: the first of a
    /// slot starts the picture, the rest draw over it, each on a depth of its own.
    fn begin<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        layer: &wgpu::TextureView,
        first: bool,
    ) -> wgpu::RenderPass<'e> {
        let (view, resolve_target) = match &self.samples {
            Some(samples) => (samples, Some(layer)),
            None => (layer, None),
        };
        let load = if first {
            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
        } else {
            wgpu::LoadOp::Load
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("vista"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }
}

impl VistaPass {
    /// `None` for a level with no viewport faces, which pays nothing for this.
    pub(crate) fn new(
        gpu: &Gpu,
        assets: &LevelAssets,
        frame_uniform: &wgpu::Buffer,
        finish: Finish,
    ) -> Result<Option<Self>, GpuError> {
        let scene = assets.scene();
        if !scene.has_viewports() {
            return Ok(None);
        }
        Ok(Some(Self {
            holes: Holes::new(gpu, scene, finish)?,
            listed: HoleList::default(),
            views: Vec::new(),
            voxels: LayerVoxels::default(),
            finish,
            frame_uniform: frame_uniform.clone(),
            pictures: None,
        }))
    }

    /// Where the frame's walk leaves the viewport faces in view.
    pub(crate) fn listed(&mut self) -> &mut HoleList {
        &mut self.listed
    }

    /// Lists the layers the listed faces show, for a player at `view`, `time`
    /// seconds in, on a target of `size`. Returns how many faces they list.
    pub(crate) fn prepare(
        &mut self,
        gpu: &Gpu,
        assets: &LevelAssets,
        (view, time): (&View, f32),
        size: (u32, u32),
    ) -> u32 {
        self.holes.prepare(gpu, &self.listed);
        if self.pictures.as_ref().map(|p| p.size) != Some(size) {
            let count = self.holes.layer_count();
            self.pictures = Some(Pictures::new(gpu, size, count, self.finish, self));
        }
        assets.vista_views(view, size, &self.listed, time, &mut self.views)
    }

    /// Lists the layers' voxels, once `voxels` is in sync for this frame and
    /// `faces` has their frames. Returns the triangles.
    pub(crate) fn prepare_voxels(
        &mut self,
        gpu: &Gpu,
        assets: &LevelAssets,
        voxels: &VoxelPass,
        faces: &FacePass,
    ) -> Result<u32, GpuError> {
        let frame_of = |i| faces.layer_frame(i);
        (self.voxels).prepare(gpu, assets, (voxels, &self.views), frame_of)
    }

    /// This frame's layers, in the order [`Self::encode`] draws them.
    pub(crate) fn views(&self) -> &[VistaView] {
        &self.views
    }

    /// Draws each slot in view into its picture, its layers being views
    /// `1 + i` of `faces` for the `i`th of [`Self::views`].
    pub(crate) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        (sky, voxels, faces): (&SkyPass, &VoxelPass, &FacePass),
    ) {
        let Some(pictures) = &self.pictures else {
            return;
        };
        for slot in 0..VIEW_SLOTS {
            let (Some(window), Some(layer)) = (self.listed.window(slot), self.holes.layer_of(slot))
            else {
                continue;
            };
            let Some(target) = pictures.layers.get(layer as usize) else {
                continue;
            };
            let (x, y, w, h) = scissor(window, pictures.size);
            let mut pass = pictures.begin(encoder, target, true);
            pass.set_scissor_rect(x, y, w, h);
            sky.draw(&mut pass);
            let of_slot = self
                .views
                .iter()
                .enumerate()
                .filter(|(_, v)| v.slot == slot);
            for (n, (i, _)) in of_slot.enumerate() {
                if n > 0 {
                    drop(pass);
                    pass = pictures.begin(encoder, target, false);
                    pass.set_scissor_rect(x, y, w, h);
                }
                self.voxels.draw(&mut pass, voxels, i);
                faces.draw_soft(&mut pass, i + 1);
                faces.draw(&mut pass, i + 1);
            }
        }
    }

    /// The listed viewport faces, each showing its slot's picture.
    pub(crate) fn draw_holes(&self, pass: &mut wgpu::RenderPass) {
        if let Some(pictures) = &self.pictures {
            self.holes.draw(pass, &pictures.bind_group);
        }
    }
}

//! The hardware renderer: the same frames as [`mscent_render::LevelRenderer`],
//! drawn on a GPU through wgpu and read back into a [`Framebuffer`].
//!
//! So far it draws the sky and each face in one flat colour. The rest of the
//! look arrives item by item (`docs/hardware-renderer.md`), each with a case in
//! `tests/parity.rs`, which holds the two renderers to the same picture.

mod device;
mod faces;
mod sky;
mod targets;

pub use device::{Gpu, GpuError};

use mscent_render::scene::SceneStats;
use mscent_render::{FrameInput, FrameRenderer, Framebuffer, LevelAssets};

use faces::FacePass;
use sky::SkyPass;
use targets::Targets;

/// A level on a GPU.
pub struct GpuRenderer {
    gpu: Gpu,
    assets: LevelAssets,
    sky: SkyPass,
    faces: FacePass,
    targets: Option<Targets>,
    /// The visible voxel chunks, kept so a frame does not allocate them again.
    chunks: Vec<u32>,
}

impl GpuRenderer {
    /// Puts `assets` on `gpu`. On failure the level comes back with the reason,
    /// for the software renderer to take.
    pub fn new(gpu: Gpu, assets: LevelAssets) -> Result<Self, (GpuError, Box<LevelAssets>)> {
        let faces = match FacePass::new(&gpu, assets.scene()) {
            Ok(faces) => faces,
            Err(why) => return Err((why, Box::new(assets))),
        };
        Ok(Self {
            sky: SkyPass::new(&gpu, assets.config()),
            faces,
            gpu,
            assets,
            targets: None,
            chunks: Vec::new(),
        })
    }

    /// The first GPU error since the last call, if any.
    pub fn take_error(&self) -> Option<String> {
        self.gpu.take_error()
    }

    /// The level again, for another renderer to take.
    pub fn into_assets(self) -> LevelAssets {
        self.assets
    }
}

impl FrameRenderer for GpuRenderer {
    /// The stats have no `pixels` or `spans`, which a GPU does not count.
    fn draw(&mut self, fb: &mut Framebuffer, frame: &FrameInput) -> Option<SceneStats> {
        let (width, height) = (fb.width(), fb.height());
        let cam = self.assets.camera(frame.view, width, height)?;
        let stats = self.assets.scene().visible_set(&cam, &mut self.chunks);
        if self.targets.as_ref().map(Targets::size) != Some((width, height)) {
            self.targets = Some(Targets::new(&self.gpu, width, height));
        }
        let targets = self.targets.as_ref()?;
        self.sky.prepare(&self.gpu, height);
        self.faces.prepare(&self.gpu, &cam, &stats.drawn);
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.sky.draw(&mut pass);
            self.faces.draw(&mut pass);
        }
        targets.copy_for_reading(&mut encoder);
        self.gpu.queue.submit([encoder.finish()]);
        targets.read_into(&self.gpu, &cam, fb)?;
        Some(stats)
    }
}

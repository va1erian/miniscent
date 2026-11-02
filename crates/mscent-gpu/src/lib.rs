//! The hardware renderer: the same frames as [`mscent_render::LevelRenderer`],
//! drawn on a GPU through wgpu and read back into a [`Framebuffer`].
//!
//! So far it draws the sky, faces, movers and actors, and a frame's voxels,
//! viewports, dynamic lights and particles are not drawn. They arrive item by item
//! (`docs/hardware-renderer.md`), each with a case in `tests/parity.rs`, which
//! holds the two renderers to the same picture.

mod actors;
mod device;
mod face_data;
mod faces;
mod lightmap;
mod sky;
mod targets;
mod textures;

pub use device::{Gpu, GpuError};

use mscent_render::scene::SceneStats;
use mscent_render::{Camera, FrameInput, FrameRenderer, Framebuffer, LevelAssets, Quality};

use actors::ActorPass;
use faces::FacePass;
use sky::SkyPass;
use targets::Targets;

/// The `Frame` of `common.wgsl`.
const FRAME_BYTES: u64 = 96;

/// A level on a GPU.
pub struct GpuRenderer {
    gpu: Gpu,
    assets: LevelAssets,
    frame_uniform: wgpu::Buffer,
    sky: SkyPass,
    faces: FacePass,
    actors: ActorPass,
    targets: Option<Targets>,
    /// The visible voxel chunks, kept so a frame does not allocate them again.
    chunks: Vec<u32>,
}

/// What every shader reads about a frame, as the bytes of `Frame`.
fn frame_bytes(assets: &LevelAssets, cam: &Camera, quality: Quality, atlas: (u32, u32)) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(FRAME_BYTES as usize);
    let matrix = cam.gpu_clip_matrix();
    for c in &matrix.cols {
        for value in [c.x, c.y, c.z, c.w] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    let fog = assets.fog();
    let color = fog.map_or(0, |f| f.color);
    let (start, end): (f32, f32) = fog.map_or((0.0, 1.0), |f| (f.start, f.end));
    bytes.extend_from_slice(&color.to_le_bytes());
    bytes.extend_from_slice(&start.to_le_bytes());
    bytes.extend_from_slice(&end.to_le_bytes());
    bytes.extend_from_slice(&u32::from(fog.is_some()).to_le_bytes());
    bytes.extend_from_slice(&u32::from(quality != Quality::Low).to_le_bytes());
    bytes.extend_from_slice(&u32::from(quality == Quality::High).to_le_bytes());
    bytes.extend_from_slice(&(atlas.0 as f32).to_le_bytes());
    bytes.extend_from_slice(&(atlas.1 as f32).to_le_bytes());
    bytes
}

impl GpuRenderer {
    /// Puts `assets` on `gpu`. On failure the level comes back with the reason,
    /// for the software renderer to take.
    pub fn new(gpu: Gpu, assets: LevelAssets) -> Result<Self, (GpuError, Box<LevelAssets>)> {
        let frame_uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame"),
            size: FRAME_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let faces = match FacePass::new(&gpu, &assets, &frame_uniform) {
            Ok(faces) => faces,
            Err(why) => return Err((why, Box::new(assets))),
        };
        Ok(Self {
            sky: SkyPass::new(&gpu, assets.config()),
            actors: ActorPass::new(&gpu, &frame_uniform),
            frame_uniform,
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
        let uniform = frame_bytes(&self.assets, &cam, frame.quality, self.faces.atlas_size);
        self.gpu
            .queue
            .write_buffer(&self.frame_uniform, 0, &uniform);
        self.sky.prepare(&self.gpu, height);
        let poses = frame.effects.poses;
        self.faces
            .prepare(&self.gpu, &self.assets, &cam, &stats.drawn, poses);
        self.actors
            .prepare(&self.gpu, &self.assets, &cam, frame.actors);
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
            self.actors.draw(&mut pass);
        }
        targets.copy_for_reading(&mut encoder);
        self.gpu.queue.submit([encoder.finish()]);
        targets.read_into(&self.gpu, &cam, fb)?;
        Some(stats)
    }
}

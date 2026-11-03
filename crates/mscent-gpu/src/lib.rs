//! The hardware renderer: the same frames as [`mscent_render::LevelRenderer`],
//! drawn on a GPU through wgpu and read back into a [`Framebuffer`].
//!
//! So far it draws the sky, faces, movers, voxel surfaces and actors, and a frame's
//! dynamic lights and particles are not drawn. A level with viewports must not come
//! here ([`GpuRenderer::cannot_draw`]). These arrive item by item
//! (`docs/hardware-renderer.md`), each with a case in `tests/parity.rs`, which holds
//! the two renderers to the same picture.

mod actors;
mod device;
mod face_data;
mod faces;
mod lightmap;
mod sky;
mod targets;
mod textures;
mod voxel_materials;
mod voxels;

pub use device::{Gpu, GpuError};

use mscent_render::scene::SceneStats;
use mscent_render::{Camera, FrameInput, Framebuffer, LevelAssets, Quality};

use actors::ActorPass;
use faces::FacePass;
use sky::SkyPass;
use targets::Targets;
use voxels::VoxelPass;

/// The `Frame` of `common.wgsl`.
const FRAME_BYTES: u64 = 96;

/// A level on a GPU. It keeps what it uploaded and borrows the level to draw it,
/// so the level can stay with its owner and with the software renderer.
pub struct GpuRenderer {
    gpu: Gpu,
    frame_uniform: wgpu::Buffer,
    sky: SkyPass,
    faces: FacePass,
    voxels: VoxelPass,
    actors: ActorPass,
    targets: Option<Targets>,
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
    /// Puts `assets` on `gpu`.
    pub fn new(gpu: Gpu, assets: &LevelAssets) -> Result<Self, GpuError> {
        let frame_uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame"),
            size: FRAME_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut voxels = VoxelPass::new(&gpu, &frame_uniform);
        voxels.sync(&gpu, assets)?;
        Ok(Self {
            faces: FacePass::new(&gpu, assets, &frame_uniform)?,
            sky: SkyPass::new(&gpu, assets.config()),
            voxels,
            actors: ActorPass::new(&gpu, &frame_uniform),
            frame_uniform,
            gpu,
            targets: None,
        })
    }

    /// What of `assets` this renderer cannot draw yet, which must go to software
    /// rather than show as a hole in the picture.
    pub fn cannot_draw(assets: &LevelAssets) -> Option<&'static str> {
        assets.scene().has_viewports().then_some("viewports")
    }

    /// The first GPU error since the last call, if any.
    pub fn take_error(&self) -> Option<String> {
        self.gpu.take_error()
    }

    /// Draws a frame of `assets`, the level this was made from, into `fb`. The
    /// stats have no `pixels` or `spans`, which a GPU does not count. `None` when
    /// the view or the size cannot make a camera, the device has gone, or the
    /// level's voxels have changed into more than this GPU can hold.
    pub fn draw(
        &mut self,
        assets: &LevelAssets,
        fb: &mut Framebuffer,
        frame: &FrameInput,
    ) -> Option<SceneStats> {
        let (width, height) = (fb.width(), fb.height());
        let cam = assets.camera(frame.view, width, height)?;
        let mut stats = assets.scene().visible_set(&cam, self.voxels.candidates());
        if self.targets.as_ref().map(Targets::size) != Some((width, height)) {
            self.targets = Some(Targets::new(&self.gpu, width, height));
        }
        let targets = self.targets.as_ref()?;
        let uniform = frame_bytes(assets, &cam, frame.quality, self.faces.atlas_size);
        self.gpu
            .queue
            .write_buffer(&self.frame_uniform, 0, &uniform);
        self.sky.prepare(&self.gpu, height);
        let poses = frame.effects.poses;
        self.faces
            .prepare(&self.gpu, assets, &cam, &stats.drawn, poses);
        self.actors.prepare(&self.gpu, assets, &cam, frame.actors);
        let voxels = self.voxels.prepare(&self.gpu, assets, &cam, &mut stats);
        voxels.ok()?;
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
            // Before faces, as software has them: a voxel pixel costs less.
            self.voxels.draw(&mut pass);
            self.faces.draw(&mut pass);
            self.actors.draw(&mut pass);
        }
        targets.copy_for_reading(&mut encoder, frame.wants_depth);
        self.gpu.queue.submit([encoder.finish()]);
        targets.read_into(&self.gpu, &cam, fb, frame.wants_depth)?;
        Some(stats)
    }
}

//! The hardware renderer: the same frames as [`mscent_render::LevelRenderer`],
//! drawn on a GPU through wgpu and read back into a [`Framebuffer`].
//!
//! So far it draws the sky, faces, movers, voxel surfaces, actors, cutouts and
//! viewports with their vistas, with the frame's dynamic lights. A frame with
//! particles is refused. These arrive item by item
//! (`docs/hardware-renderer.md`), each with a case in `tests/parity.rs`, which holds
//! the two renderers to the same picture.

mod actors;
mod common;
mod device;
mod dyn_lights;
mod face_data;
mod face_pipeline;
mod face_views;
mod faces;
mod finish;
mod holes;
mod lightmap;
mod resolve;
mod sky;
mod targets;
mod textures;
mod vista_voxels;
mod vistas;
mod voxel_materials;
mod voxels;

pub use device::{Gpu, GpuError};
pub use finish::Finish;

use mscent_render::scene::SceneStats;
use mscent_render::{
    Camera, DynLight, Fog, FrameInput, Framebuffer, LevelAssets, Quality, VistaView,
};

use actors::ActorPass;
use faces::FacePass;
use resolve::DepthResolve;
use sky::SkyPass;
use targets::Targets;
use vistas::VistaPass;
use voxels::VoxelPass;

/// The `Frame` of `common.wgsl`.
const FRAME_BYTES: u64 = 96 + dyn_lights::BYTES;

/// A level on a GPU. It keeps what it uploaded and borrows the level to draw it,
/// so the level can stay with its owner and with the software renderer.
pub struct GpuRenderer {
    gpu: Gpu,
    finish: Finish,
    /// For a multisampled finish, which needs its depth resolved to read it.
    resolve: Option<DepthResolve>,
    frame_uniform: wgpu::Buffer,
    sky: SkyPass,
    faces: FacePass,
    voxels: VoxelPass,
    actors: ActorPass,
    /// `None` for a level with no viewport faces.
    vistas: Option<VistaPass>,
    targets: Option<Targets>,
}

/// What every shader reads about a frame seen by `cam`, as the bytes of `Frame`.
fn frame_bytes(
    cam: &Camera,
    (fog, lights): (Option<Fog>, &[DynLight]),
    quality: Quality,
    atlas: (u32, u32),
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(FRAME_BYTES as usize);
    let matrix = cam.gpu_clip_matrix();
    for c in &matrix.cols {
        for value in [c.x, c.y, c.z, c.w] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
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
    dyn_lights::push(&mut bytes, lights);
    bytes
}

impl GpuRenderer {
    /// Puts `assets` on `gpu`, to draw the software renderer's picture.
    pub fn new(gpu: Gpu, assets: &LevelAssets) -> Result<Self, GpuError> {
        Self::with_finish(gpu, assets, Finish::Reference)
    }

    /// Puts `assets` on `gpu`, to draw frames with `finish`.
    pub fn with_finish(gpu: Gpu, assets: &LevelAssets, finish: Finish) -> Result<Self, GpuError> {
        let frame_uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame"),
            size: FRAME_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut voxels = VoxelPass::new(&gpu, &frame_uniform, finish);
        voxels.sync(&gpu, assets)?;
        Ok(Self {
            faces: FacePass::new(&gpu, assets, &frame_uniform, finish)?,
            sky: SkyPass::new(&gpu, assets.config(), finish),
            voxels,
            actors: ActorPass::new(&gpu, &frame_uniform, finish),
            vistas: VistaPass::new(&gpu, assets, &frame_uniform, finish)?,
            resolve: (finish.samples() > 1).then(|| DepthResolve::new(&gpu)),
            finish,
            frame_uniform,
            gpu,
            targets: None,
        })
    }

    /// How this draws its frames.
    pub fn finish(&self) -> Finish {
        self.finish
    }

    /// The first GPU error since the last call, if any.
    pub fn take_error(&self) -> Option<String> {
        self.gpu.take_error()
    }

    /// Draws a frame of `assets`, the level this was made from, into `fb`. The
    /// stats have no `pixels` or `spans`, which a GPU does not count. `None` when
    /// the view or the size cannot make a camera, the device has gone, or the
    /// level's voxels have changed into more than this GPU can hold. `None` too
    /// for a frame with particles, which this does not draw yet, rather than a
    /// frame without them.
    pub fn draw(
        &mut self,
        assets: &LevelAssets,
        fb: &mut Framebuffer,
        frame: &FrameInput,
    ) -> Option<SceneStats> {
        if frame.effects.particles.is_some() {
            return None;
        }
        let (width, height) = (fb.width(), fb.height());
        let cam = assets.camera(frame.view, width, height)?;
        let (scene, chunks) = (assets.scene(), self.voxels.candidates());
        let mut stats = match self.vistas.as_mut() {
            Some(vistas) => scene.visible_set_with_holes(&cam, chunks, vistas.listed()),
            None => scene.visible_set(&cam, chunks),
        };
        if self.targets.as_ref().map(Targets::size) != Some((width, height)) {
            let size = (width, height);
            let targets = Targets::new(&self.gpu, size, self.finish, self.resolve.as_ref());
            self.targets = Some(targets);
        }
        let targets = self.targets.as_ref()?;
        let (quality, atlas) = (frame.quality, self.faces.atlas_size);
        let uniform = frame_bytes(&cam, (assets.fog(), frame.effects.lights), quality, atlas);
        self.gpu
            .queue
            .write_buffer(&self.frame_uniform, 0, &uniform);
        self.sky.prepare(&self.gpu, height);
        let (poses, lights) = (frame.effects.poses, frame.effects.lights);
        let layers = match self.vistas.as_mut() {
            Some(vistas) => {
                let at = (frame.view, frame.effects.vista_time);
                stats.vista_faces = vistas.prepare(&self.gpu, assets, at, (width, height));
                vistas.views()
            }
            None => &[],
        };
        // A layer's sealed room has its own fog, and no dynamic light reaches it.
        let layer_frame = |v: &VistaView| frame_bytes(&v.cam, (v.fog, &[]), quality, atlas);
        let visible = (&cam, &stats.drawn[..]);
        let effects = (poses, lights);
        let faces = (self.faces).prepare(&self.gpu, assets, visible, effects, layers, layer_frame);
        faces.ok()?;
        self.actors
            .prepare(&self.gpu, assets, &cam, frame.actors, lights);
        let voxels = self
            .voxels
            .prepare(&self.gpu, assets, &cam, lights, &mut stats);
        voxels.ok()?;
        if let Some(vistas) = self.vistas.as_mut() {
            let layered = vistas.prepare_voxels(&self.gpu, assets, &self.voxels, &self.faces);
            stats.vista_triangles = layered.ok()?;
        }
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        if let Some(vistas) = &self.vistas {
            vistas.encode(&mut encoder, (&self.sky, &self.voxels, &self.faces));
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(targets.color_attachment())],
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
            self.faces.draw_soft(&mut pass, 0);
            self.faces.draw(&mut pass, 0);
            self.actors.draw(&mut pass);
            // Last of the solid things, so a vista shows only where they leave it.
            if let Some(vistas) = &self.vistas {
                vistas.draw_holes(&mut pass);
            }
        }
        targets.copy_for_reading(&mut encoder, frame.wants_depth, self.resolve.as_ref());
        self.gpu.queue.submit([encoder.finish()]);
        targets.read_into(&self.gpu, &cam, fb, frame.wants_depth)?;
        Some(stats)
    }
}

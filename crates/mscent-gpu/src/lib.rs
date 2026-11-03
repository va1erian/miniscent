//! The hardware renderer: the same frames as [`mscent_render::LevelRenderer`],
//! drawn on a GPU through wgpu and read back into a [`Framebuffer`].
//!
//! So far it draws the sky, faces, movers, voxel surfaces and actors, with the
//! frame's dynamic lights, and its particles over them. A level with viewports
//! must not come here ([`GpuRenderer::cannot_draw`]). These arrive item by item
//! (`docs/hardware-renderer.md`), each with a case in `tests/parity.rs`, which holds
//! the two renderers to the same picture.

mod actors;
mod common;
mod device;
mod dyn_lights;
mod face_data;
mod faces;
mod finish;
mod lightmap;
mod particle_data;
mod particles;
mod resolve;
mod sky;
mod targets;
mod textures;
mod voxel_materials;
mod voxels;

pub use device::{Gpu, GpuError};
pub use finish::Finish;

use mscent_render::scene::SceneStats;
use mscent_render::{Camera, FrameInput, Framebuffer, LevelAssets, Quality};

use actors::ActorPass;
use faces::FacePass;
use particles::ParticlePass;
use resolve::DepthResolve;
use sky::SkyPass;
use targets::Targets;
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
    particles: ParticlePass,
    targets: Option<Targets>,
}

/// What every shader reads about a frame, as the bytes of `Frame`.
fn frame_bytes(
    assets: &LevelAssets,
    cam: &Camera,
    frame: &FrameInput,
    atlas: (u32, u32),
) -> Vec<u8> {
    let quality = frame.quality;
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
    dyn_lights::push(&mut bytes, frame.effects.lights);
    bytes
}

impl GpuRenderer {
    /// Puts `assets` on `gpu`, to draw the software renderer's picture.
    pub fn new(gpu: Gpu, assets: &LevelAssets) -> Result<Self, GpuError> {
        Self::with_finish(gpu, assets, Finish::Reference)
    }

    /// Puts `assets` on `gpu`, to draw frames with `finish`. A level wgpu would
    /// refuse, or a lost device, is an error here and not in [`Self::take_error`].
    pub fn with_finish(gpu: Gpu, assets: &LevelAssets, finish: Finish) -> Result<Self, GpuError> {
        gpu.clone().checked(|| Self::build(gpu, assets, finish))
    }

    fn build(gpu: Gpu, assets: &LevelAssets, finish: Finish) -> Result<Self, GpuError> {
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
            particles: ParticlePass::new(&gpu, finish),
            resolve: (finish.samples() > 1).then(|| DepthResolve::new(&gpu)),
            finish,
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

    /// How this draws its frames.
    pub fn finish(&self) -> Finish {
        self.finish
    }

    /// `made`, or `None` with why kept for [`Self::take_error`].
    fn reported<T>(&self, made: Result<T, GpuError>) -> Option<T> {
        made.map_err(|why| self.gpu.report(why.to_string())).ok()
    }

    /// The first GPU error since the last call, if any: why a [`Self::draw`]
    /// gave `None`, when it was the GPU's doing.
    pub fn take_error(&self) -> Option<String> {
        self.gpu.take_error()
    }

    /// Draws a frame of `assets`, the level this was made from, into `fb`. The
    /// stats have no `pixels` or `spans`, which a GPU does not count. `None` when
    /// the view or the size cannot make a camera, the device has gone, or the
    /// level's voxels have changed into more than this GPU can hold, or the frame,
    /// its actors or its particles are more than it can; [`Self::take_error`]
    /// says which of the last four it was.
    pub fn draw(
        &mut self,
        assets: &LevelAssets,
        fb: &mut Framebuffer,
        frame: &FrameInput,
    ) -> Option<SceneStats> {
        if let Some(why) = self.gpu.lost() {
            return self.reported(Err(GpuError::Lost(why)));
        }
        let (width, height) = (fb.width(), fb.height());
        let cam = assets.camera(frame.view, width, height)?;
        let mut stats = assets.scene().visible_set(&cam, self.voxels.candidates());
        if self.targets.as_ref().map(Targets::size) != Some((width, height)) {
            let size = (width, height);
            let (finish, resolve) = (self.finish, self.resolve.as_ref());
            let targets = self
                .gpu
                .checked(|| Targets::new(&self.gpu, size, finish, resolve));
            self.targets = self.reported(targets);
        }
        let targets = self.targets.as_ref()?;
        let uniform = frame_bytes(assets, &cam, frame, self.faces.atlas_size);
        self.gpu
            .queue
            .write_buffer(&self.frame_uniform, 0, &uniform);
        self.sky.prepare(&self.gpu, height);
        let (poses, lights) = (frame.effects.poses, frame.effects.lights);
        self.faces
            .prepare(&self.gpu, assets, &cam, &stats.drawn, poses, lights);
        let actors = self
            .actors
            .prepare(&self.gpu, assets, &cam, frame.actors, lights);
        self.reported(actors)?;
        let voxels = self.gpu.checked(|| {
            self.voxels
                .prepare(&self.gpu, assets, &cam, lights, &mut stats)
        });
        self.reported(voxels)?;
        let particles = self
            .particles
            .prepare(&self.gpu, &cam, frame.effects.particles);
        let particles = self.reported(particles)?;
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
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
            self.faces.draw(&mut pass);
            self.actors.draw(&mut pass);
        }
        if particles {
            let resolve = self.resolve.as_ref();
            self.particles
                .draw(&self.gpu, &mut encoder, targets, resolve);
        }
        targets.copy_for_reading(&mut encoder, frame.wants_depth, self.resolve.as_ref());
        self.gpu.queue.submit([encoder.finish()]);
        targets.read_into(&self.gpu, &cam, fb, frame.wants_depth)?;
        Some(stats)
    }
}

//! The hardware renderer: the same frames as [`mscent_render::LevelRenderer`],
//! drawn on a GPU through wgpu and read back into a [`Framebuffer`].
//!
//! So far it draws the sky, faces, movers, voxel surfaces, actors, enemy models,
//! cutouts and viewports with their vistas, with the frame's dynamic lights, and its
//! particles over them. These arrive item by item
//! (`docs/hardware-renderer.md`), each with a case in `tests/parity.rs`, which holds
//! the two renderers to the same picture.

mod actors;
mod common;
mod device;
mod dyn_lights;
mod effects;
mod face_data;
mod face_pipeline;
mod face_views;
mod faces;
mod finish;
mod holes;
mod lightmap;
mod models;
mod mover_casters;
mod particle_data;
mod particles;
mod present;
mod resolve;
mod shadow_lights;
mod shadow_maps;
mod shadows;
mod sky;
mod targets;
mod textures;
mod vista_voxels;
mod vistas;
mod voxel_materials;
mod voxels;

pub use device::{Gpu, GpuError};
pub use effects::GpuEffects;
pub use finish::Finish;
pub use present::{Overlay, Presenter};

use mscent_math::Mat4;
use mscent_render::scene::{SceneStats, eye_of};
use mscent_render::{
    Camera, DynLight, Fog, FrameInput, Framebuffer, LevelAssets, Quality, VistaView,
};

use actors::ActorPass;
use faces::FacePass;
use models::ModelPass;
use particles::ParticlePass;
use resolve::DepthResolve;
use shadow_lights::Slots;
use shadows::{Casters, ShadowPass};
use sky::SkyPass;
use targets::Targets;
use vistas::VistaPass;
use voxels::VoxelPass;

/// The `Frame` of `common.wgsl`: its scalars, padded to the 16 its lights need.
pub(crate) const FRAME_BYTES: u64 = 112 + dyn_lights::BYTES + 16;

/// A level on a GPU. It keeps what it uploaded and borrows the level to draw it,
/// so the level can stay with its owner and with the software renderer.
pub struct GpuRenderer {
    gpu: Gpu,
    finish: Finish,
    /// What a caller has set for the GPU-only looks; `None` follows the quality.
    effects: Option<GpuEffects>,
    /// For a multisampled finish, which needs its depth resolved to read it.
    resolve: Option<DepthResolve>,
    frame_uniform: wgpu::Buffer,
    sky: SkyPass,
    faces: FacePass,
    voxels: VoxelPass,
    models: ModelPass,
    /// What casts shadows: every model, in view or not, and those not drawn.
    casters: ModelPass,
    actors: ActorPass,
    shadows: ShadowPass,
    particles: ParticlePass,
    /// `None` for a level with no viewport faces.
    vistas: Option<VistaPass>,
    targets: Option<Targets>,
}

/// What every shader reads about a frame whose vertices `clip` takes to clip
/// space, as the bytes of `Frame`.
pub(crate) fn frame_bytes(
    clip: Mat4,
    (fog, lights): (Option<Fog>, &[DynLight]),
    (quality, effects): (Quality, GpuEffects),
    (atlas, slots): ((u32, u32), Slots),
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(FRAME_BYTES as usize);
    for c in &clip.cols {
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
    effects.push(&mut bytes);
    bytes.extend_from_slice(&(atlas.0 as f32).to_le_bytes());
    bytes.extend_from_slice(&(atlas.1 as f32).to_le_bytes());
    bytes.resize(112, 0);
    dyn_lights::push(&mut bytes, lights);
    slots.push(&mut bytes);
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
        let shadows = ShadowPass::new(&gpu);
        Ok(Self {
            faces: FacePass::new(&gpu, assets, &frame_uniform, (finish, &shadows.maps))?,
            shadows,
            sky: SkyPass::new(&gpu, assets.config(), finish),
            voxels,
            models: ModelPass::new(finish, false),
            casters: ModelPass::new(finish, true),
            actors: ActorPass::new(&gpu, &frame_uniform, finish),
            particles: ParticlePass::new(&gpu, finish),
            vistas: VistaPass::new(&gpu, assets, &frame_uniform, finish)?,
            resolve: (finish.samples() > 1).then(|| DepthResolve::new(&gpu)),
            finish,
            effects: None,
            frame_uniform,
            gpu,
            targets: None,
        })
    }

    /// How this draws its frames.
    pub fn finish(&self) -> Finish {
        self.finish
    }

    /// Sets which GPU-only looks (shadows, parallax) frames draw, whatever their
    /// quality; `None` goes back to following it, on at High.
    pub fn set_effects(&mut self, effects: Option<GpuEffects>) {
        self.effects = effects;
    }

    /// The looks a frame of `quality` draws.
    pub fn effects_at(&self, quality: Quality) -> GpuEffects {
        self.effects.unwrap_or_else(|| GpuEffects::at(quality))
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
        let size = (fb.width(), fb.height());
        let (stats, cam, mut encoder) = self.encode(assets, size, frame)?;
        let (targets, resolve) = (self.targets.as_ref()?, self.resolve.as_ref());
        targets.copy_for_reading(&mut encoder, frame.wants_depth, resolve);
        self.gpu.queue.submit([encoder.finish()]);
        targets.read_into(&self.gpu, &cam, fb, frame.wants_depth)?;
        Some(stats)
    }

    /// Draws a frame of `assets` at `size` and keeps it on the GPU, for measuring
    /// what a frame costs with nothing read back or shown; [`Gpu::wait`] waits
    /// for it to be done. `None` as for [`Self::draw`].
    pub fn draw_unread(
        &mut self,
        assets: &LevelAssets,
        size: (u32, u32),
        frame: &FrameInput,
    ) -> Option<SceneStats> {
        let (stats, _, encoder) = self.encode(assets, size, frame)?;
        self.gpu.queue.submit([encoder.finish()]);
        Some(stats)
    }

    /// Draws a frame of `assets` at `size` and shows it in `presenter`'s window
    /// with `overlay` over it, reading nothing back. `None` as for [`Self::draw`],
    /// and for a window that cannot take the frame.
    pub fn draw_and_present(
        &mut self,
        assets: &LevelAssets,
        size: (u32, u32),
        frame: &FrameInput,
        to: (&mut Presenter, &Overlay),
    ) -> Option<SceneStats> {
        let (stats, _, mut encoder) = self.encode(assets, size, frame)?;
        let targets = self.targets.as_ref()?;
        let (presenter, overlay) = to;
        let scene = (targets.finished(), size);
        let shown = presenter.encode(&self.gpu, &mut encoder, scene, overlay);
        let shown = self.reported(shown)?;
        self.gpu.queue.submit([encoder.finish()]);
        if let Some(texture) = shown {
            self.gpu.queue.present(texture);
        }
        Some(stats)
    }

    /// Lists and encodes a frame of `size`, up to where it is read or shown.
    fn encode(
        &mut self,
        assets: &LevelAssets,
        (width, height): (u32, u32),
        frame: &FrameInput,
    ) -> Option<(SceneStats, Camera, wgpu::CommandEncoder)> {
        if let Some(why) = self.gpu.lost() {
            return self.reported(Err(GpuError::Lost(why)));
        }
        let cam = assets.camera(frame.view, width, height)?;
        let (scene, chunks) = (assets.scene(), self.voxels.candidates());
        let mut stats = match self.vistas.as_mut() {
            Some(vistas) => scene.visible_set_with_holes(&cam, chunks, vistas.listed()),
            None => scene.visible_set(&cam, chunks),
        };
        if self.targets.as_ref().map(Targets::size) != Some((width, height)) {
            let size = (width, height);
            let (finish, resolve) = (self.finish, self.resolve.as_ref());
            let targets = self
                .gpu
                .checked(|| Targets::new(&self.gpu, size, finish, resolve));
            self.targets = self.reported(targets);
        }
        let targets = self.targets.as_ref()?;
        let (quality, atlas) = (frame.quality, self.faces.atlas_size);
        let effects = self.effects_at(quality);
        let looks = (quality, effects);
        let clip = cam.gpu_clip_matrix();
        let (slots, holders) = match effects.shadows {
            true => self
                .shadows
                .pick(&self.gpu, frame.effects.lights, eye_of(&cam)),
            false => (Slots::NONE, Default::default()),
        };
        let lit = (assets.fog(), frame.effects.lights);
        let uniform = frame_bytes(clip, lit, looks, (atlas, slots));
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
        let layer_frame = |v: &VistaView| {
            let clip = v.cam.gpu_clip_matrix();
            frame_bytes(clip, (v.fog, &[]), looks, (atlas, Slots::NONE))
        };
        let visible = (&cam, &stats.drawn[..]);
        let moved = (poses, lights);
        let faces = (self.faces).prepare(&self.gpu, assets, visible, moved, layers, layer_frame);
        self.reported(faces)?;
        let actors = self
            .actors
            .prepare(&self.gpu, assets, &cam, frame.actors, lights);
        self.reported(actors)?;
        let voxels = self.gpu.checked(|| {
            self.voxels
                .prepare(&self.gpu, assets, &cam, lights, &mut stats)
        });
        self.reported(voxels)?;
        let models = self.gpu.checked(|| {
            let seen = (frame.effects.models, lights);
            (self.models).prepare(&self.gpu, assets, (&cam, quality), seen, &mut stats)
        });
        self.reported(models)?;
        if let Some(vistas) = self.vistas.as_mut() {
            let layered = vistas.prepare_voxels(&self.gpu, assets, &self.voxels, &self.faces);
            stats.vista_triangles = self.reported(layered)?;
        }
        if holders.iter().any(Option::is_some) {
            let seen = (frame.effects.models, lights);
            let listed = self.gpu.checked(|| {
                let mut unused = SceneStats::default();
                (self.casters).prepare(&self.gpu, assets, (&cam, quality), seen, &mut unused)
            });
            self.reported(listed)?;
        }
        let particles = self
            .particles
            .prepare(&self.gpu, &cam, frame.effects.particles);
        let particles = self.reported(particles)?;
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        if holders.iter().any(Option::is_some) {
            let casters = Casters {
                actors: self.actors.casters(),
                models: self.casters.casters(),
                movers: self.faces.casters(),
            };
            self.shadows
                .encode(&self.gpu, &mut encoder, &holders, &casters);
        }
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
            // First of the solid things, as software has them: what stands in a
            // room covers its walls.
            self.models.draw(&mut pass);
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
        if particles {
            let resolve = self.resolve.as_ref();
            self.particles
                .draw(&self.gpu, &mut encoder, targets, resolve);
        }
        Some((stats, cam, encoder))
    }
}

//! One frame: what is asked for, and how the software renderer draws it.
//! Visibility and projection run once, then the bands fill in parallel.

use std::sync::PoisonError;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{Actor, Effects, LevelRenderer, Quality, View};
use crate::dynlight::MAX_DYN_LIGHTS;
use crate::framebuffer::Framebuffer;
use crate::particles::draw_particles;
use crate::scene::{SceneStats, eye_of};
use crate::voxel::fill::{FillContext, fill_voxels};

/// Everything that changes from frame to frame. A feature added to a frame
/// goes here, where every renderer sees it.
#[derive(Clone, Copy)]
pub struct FrameInput<'a> {
    pub view: &'a View,
    pub quality: Quality,
    pub actors: &'a [Actor],
    pub effects: Effects<'a>,
    /// Whether anything reads the frame's depth after it is drawn. Software has it
    /// anyway; a GPU has to copy it back, which is half the cost of a readback.
    pub wants_depth: bool,
}

/// Something that can draw a level's frames.
pub trait FrameRenderer {
    /// Leaves the frame's colour in `fb`, and its depth if `frame` asks for it.
    /// `None` when the view or the framebuffer size cannot make a camera.
    fn draw(&mut self, fb: &mut Framebuffer, frame: &FrameInput) -> Option<SceneStats>;
}

impl FrameRenderer for LevelRenderer {
    fn draw(&mut self, fb: &mut Framebuffer, frame: &FrameInput) -> Option<SceneStats> {
        self.draw_frame(fb, frame)
    }
}

impl LevelRenderer {
    /// Draws one frame: visibility and projection once, then sky, level, actors
    /// and particles in parallel bands. Of the lights only the first
    /// [`MAX_DYN_LIGHTS`] count.
    pub(super) fn draw_frame(
        &self,
        fb: &mut Framebuffer,
        input: &FrameInput,
    ) -> Option<SceneStats> {
        let FrameInput {
            view,
            quality,
            actors,
            effects,
            ..
        } = *input;
        let assets = &self.assets;
        let lights = &effects.lights[..effects.lights.len().min(MAX_DYN_LIGHTS)];
        let cam = assets.camera(view, fb.width(), fb.height())?;
        let mut slot = self.frame.lock().unwrap_or_else(PoisonError::into_inner);
        let frame = self.frame_for(&mut slot, fb.width(), fb.height())?;
        let mut stats = assets.scene.list_frame_voxels(
            &cam,
            &mut frame.faces,
            &mut frame.voxels.candidates,
            &mut frame.holes,
        );
        let size = (fb.width(), fb.height());
        let time = effects.vista_time;
        stats.vista_faces = self.list_vistas(view, size, &frame.holes, time, &mut frame.layers);
        frame
            .voxels
            .prepare(assets.scene.voxels(), &cam, eye_of(&cam), |leaf| {
                let lit = lights.iter().enumerate();
                lit.fold(0u8, |m, (i, l)| {
                    if l.is_lit() && l.reaches_leaf(leaf) {
                        m | (1 << i)
                    } else {
                        m
                    }
                })
            });
        stats.voxel_triangles = frame.voxels.triangles;
        stats.vista_triangles = frame.layers.iter().map(|l| l.voxel_triangles()).sum();
        self.list_movers(&cam, effects.poses, &mut frame.movers);
        self.list_actors(&cam, actors, lights, frame);
        let (faces, meshes, movers) = (&frame.faces, &frame.actors, &frame.movers);
        let voxels = &frame.voxels;
        let (holes, layers) = (&frame.holes, &frame.layers);
        let (pixels, spans) = (AtomicU32::new(0), AtomicU32::new(0));
        let vista_pixels = AtomicU32::new(0);
        let eye = eye_of(&cam);
        let texture_of = |id: u16| assets.scene.texture_of(id);
        let voxel_fill = FillContext {
            scene: assets.scene.voxels(),
            cam: &cam,
            eye,
            fog: assets.fog(),
            bump: quality != Quality::Low,
            specular: quality == Quality::High,
            lights,
            spec: &assets.spec,
            texture_of: &texture_of,
        };
        let cfg = assets.config();
        frame.tiles.pass(&|band: &mut Framebuffer| {
            band.clear_sky(cfg.sky_top, cfg.sky_bottom);
            // First, so the z-buffer saves shading whatever a door covers.
            let moved = self.fill_movers(band, movers, quality);
            // Next, for the same reason: a voxel pixel costs less than a wall's.
            let vox = fill_voxels(band, voxels, &voxel_fill);
            let st = self.fill_level(band, faces, quality, eye, lights);
            assets.meshes.fill(band, meshes, assets.fog());
            // Last of the solid things, so it shades only what they left showing.
            let sky = self.fill_vistas(band, holes, layers, quality);
            if let Some((draws, bank)) = effects.particles {
                draw_particles(band, draws, bank);
            }
            vista_pixels.fetch_add(sky.pixels, Ordering::Relaxed);
            let drawn = moved.pixels + vox.pixels + st.pixels + sky.pixels;
            pixels.fetch_add(drawn, Ordering::Relaxed);
            spans.fetch_add(
                moved.spans + vox.spans + st.spans + sky.spans,
                Ordering::Relaxed,
            );
        });
        frame.tiles.composite(fb).ok()?;
        stats.pixels = pixels.into_inner();
        stats.spans = spans.into_inner();
        stats.vista_pixels = vista_pixels.into_inner();
        Some(stats)
    }
}

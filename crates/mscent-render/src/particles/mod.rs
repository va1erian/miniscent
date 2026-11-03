//! Particle drawing: sprites and streaks composited over a finished scene.
//!
//! The simulation builds a [`ParticleDraws`] list; [`draw_particles`] draws it
//! into a [`Framebuffer`] or one band of it. Particles test depth against the
//! scene but never write it. The function keeps no state and clips every draw
//! to the rows the framebuffer holds, so the same list drawn through any number
//! of bands gives the pixels of one whole frame.
//!
//! Draws arrive from untrusted level data, so every value is checked: a NaN,
//! infinite, negative or enormous number draws nothing or is clipped, and
//! nothing here allocates.

mod bank;
mod beam;
mod decal;
mod kernels;
mod layer;
mod layer_kernels;
pub mod plan;
mod sprite;

pub use bank::SpriteBank;
pub use beam::streak_from_world;
pub use decal::{DecalDraw, DecalPoint, DecalVertex, MAX_DECAL_POINTS, decal_from_world};

use crate::camera::ScreenPoint;
use crate::framebuffer::Framebuffer;
use crate::simd::Level;
use kernels::Dest;

/// How a sprite combines with what is under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// Add the tint, saturating. Order does not matter.
    Add,
    /// Premultiplied "over". The caller sorts back to front.
    Over,
}

/// Which generated mask a sprite uses. An index past the last variant wraps
/// (modulo), so any value is valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mask {
    Soft,
    /// One of 4 noisy puffs.
    Puff(u8),
    Ring,
    /// One of the 8 frames of the fireball flipbook.
    Fire(u8),
}

/// A round sprite. `tint` is `0xAARRGGBB`, premultiplied.
#[derive(Clone, Copy, Debug)]
pub struct SpriteDraw {
    pub x: f32,
    pub y: f32,
    /// Radius in pixels. Drawn radii are capped at 512 so a row fits the fixed
    /// scratch; a larger sprite is drawn at that size.
    pub radius: f32,
    /// `1/z` of the sprite's centre, compared with the scene's.
    pub inv_z: f32,
    pub tint: u32,
    pub mask: Mask,
}

/// A line of 1 to 5 pixels (`width` is clamped), additive, depth tested along
/// its length. The tint is `0xAARRGGBB`; only the colour is used.
#[derive(Clone, Copy, Debug)]
pub struct StreakDraw {
    pub a: ScreenPoint,
    pub b: ScreenPoint,
    pub width: u8,
    pub tint: u32,
}

/// Everything to draw this frame, in the order it is drawn: decals, the
/// half-res layer (`layer_over`, then `layer_add`), `over`, `add`, streaks.
#[derive(Clone, Debug, Default)]
pub struct ParticleDraws {
    /// Scorch marks, multiplied into the scene before any sprite.
    pub decals: Vec<DecalDraw>,
    /// "Over" sprites drawn into the half-res layer, back to front as given.
    /// The layer goes over the decals and under every full-size sprite, so
    /// these should be the farthest ones. A band that cannot hold the layer
    /// (see `layer.rs`) draws them at full size in its place.
    pub layer_over: Vec<SpriteDraw>,
    /// Additive sprites drawn into the layer, in any order.
    pub layer_add: Vec<SpriteDraw>,
    /// "Over" sprites, back to front as given.
    pub over: Vec<SpriteDraw>,
    /// Additive sprites, in any order.
    pub add: Vec<SpriteDraw>,
    pub streaks: Vec<StreakDraw>,
}

impl ParticleDraws {
    /// Lists that hold `n` particle draws each, and `decals` decals, before
    /// they would grow.
    pub fn with_capacity(n: usize, decals: usize) -> Self {
        Self {
            decals: Vec::with_capacity(decals),
            layer_over: Vec::with_capacity(n),
            layer_add: Vec::with_capacity(n),
            over: Vec::with_capacity(n),
            add: Vec::with_capacity(n),
            streaks: Vec::with_capacity(n),
        }
    }

    /// Empty every list, keeping their capacity.
    pub fn clear(&mut self) {
        self.decals.clear();
        self.layer_over.clear();
        self.layer_add.clear();
        self.over.clear();
        self.add.clear();
        self.streaks.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.decals.is_empty()
            && self.layer_over.is_empty()
            && self.layer_add.is_empty()
            && self.over.is_empty()
            && self.add.is_empty()
            && self.streaks.is_empty()
    }
}

/// Draw the list into the rows `fb` holds: decals, then the layer, then "over"
/// sprites, then additive ones, then streaks. Uses the fastest kernels this CPU
/// supports.
pub fn draw_particles(fb: &mut Framebuffer, draws: &ParticleDraws, bank: &SpriteBank) {
    draw_particles_with(Level::best(), fb, draws, bank);
}

/// [`draw_particles`] on a chosen kernel level; an unsupported level runs the
/// scalar code. Every level gives the same pixels.
pub fn draw_particles_with(
    level: Level,
    fb: &mut Framebuffer,
    draws: &ParticleDraws,
    bank: &SpriteBank,
) {
    for d in &draws.decals {
        decal::draw_decal(fb, d, bank);
    }
    // A band the layer cannot serve draws its layered sprites in place, at
    // full size, in the same relative order.
    let layered = layer::draw(level, fb, draws, bank);
    let (full_over, full_add): (&[SpriteDraw], &[SpriteDraw]) = if layered {
        (&[], &[])
    } else {
        (&draws.layer_over, &draws.layer_add)
    };
    for s in full_over.iter().chain(&draws.over) {
        sprite::draw_sprite(level, fb, s, (Blend::Over, Dest::Frame), bank);
    }
    for s in full_add.iter().chain(&draws.add) {
        sprite::draw_sprite(level, fb, s, (Blend::Add, Dest::Frame), bank);
    }
    for st in &draws.streaks {
        beam::draw_streak(fb, st);
    }
}

#[cfg(test)]
#[path = "particles_tests.rs"]
mod particles_tests;

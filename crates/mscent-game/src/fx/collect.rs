//! From live particles to the renderer's screen-space draw lists: project,
//! pick the colour for the particle's age, and apply fog per particle, which
//! is one colour change instead of a per-pixel cost.

use mscent_render::particles::{Mask, ParticleDraws, SpriteDraw, streak_from_world};
use mscent_render::shade::{lerp_rgb, mul_rgb as scale_rgb};
use mscent_render::{Camera, Fog};

use super::class::{Blend, ParticleClass, Prim, PuffShape};
use super::{Fx, LiveParticle};

/// Sparks this near draw a pixel wider per unit of closeness.
const SPARK_WIDTH_DEPTH: f32 = 4.0;
const MAX_SPARK_WIDTH: f32 = 3.0;
/// Puff masks in the bank, and frames of the fire flipbook.
const PUFFS: u8 = 4;
const FIRE_FRAMES: f32 = 8.0;

/// The level's fog as particles use it.
pub struct FogView(pub Option<Fog>);

impl FogView {
    /// Scale `0..=256` of what is left of a particle at `depth`; `None` when
    /// it is behind the eye or lost in the fog, so it costs nothing to draw.
    pub fn fade(&self, depth: f32) -> Option<u32> {
        if !depth.is_finite() || depth <= 0.0 {
            return None;
        }
        let Some(fog) = self.0 else {
            return Some(256);
        };
        let k = (256.0 - fog.amount(depth)) as u32;
        (k > 0).then_some(k)
    }

    /// "Over" tints lerp toward the fog colour, keeping their alpha, because
    /// an opaque puff in fog looks like fog. Additive ones just fade to black.
    fn over_tint(&self, c: u32, k: u32) -> u32 {
        let Some(fog) = self.0 else { return c };
        let target = scale_rgb(fog.color, (c >> 24) * 256 / 255);
        (c & 0xff00_0000) | (lerp_rgb(c, target, 256 - k.min(256)) & 0x00ff_ffff)
    }
}

/// Appends every live particle's draw to `out`.
pub fn particles(
    fx: &Fx,
    classes: &[ParticleClass],
    cam: &Camera,
    fog: &FogView,
    out: &mut ParticleDraws,
) {
    fx.for_each(|p| {
        if let Some(class) = classes.get(usize::from(p.class.0)) {
            particle(&p, class, cam, fog, out);
        }
    });
}

fn particle(
    p: &LiveParticle,
    class: &ParticleClass,
    cam: &Camera,
    fog: &FogView,
    out: &mut ParticleDraws,
) {
    let c = cam.world_to_camera(p.pos);
    let Some(fade) = fog.fade(-c.z) else { return };
    let tint = tint_of(p, class);
    let tint = match class.blend {
        Blend::Add => scale_rgb(tint, fade),
        Blend::Over => fog.over_tint(tint, fade),
    };
    let shape = match class.prim {
        Prim::Spark => {
            let tail = p.pos - p.vel * class.stretch;
            let width = (SPARK_WIDTH_DEPTH / -c.z).clamp(1.0, MAX_SPARK_WIDTH) as u8;
            if let Some(s) = streak_from_world(cam, tail, p.pos, width, tint) {
                out.streaks.push(s);
            }
            return;
        }
        Prim::Puff(shape) => mask_of(shape, p),
        Prim::Shard => Mask::Soft,
    };
    if c.z >= -cam.near() {
        return;
    }
    let Some(s) = cam.project(c) else { return };
    let size = class.size.0 + (class.size.1 - class.size.0) * p.t;
    let sprite = SpriteDraw {
        x: s.x,
        y: s.y,
        radius: size * cam.focal() * s.inv_z,
        inv_z: s.inv_z,
        tint,
        mask: shape,
    };
    match class.blend {
        Blend::Add => out.add.push(sprite),
        Blend::Over => out.over.push(sprite),
    }
}

fn mask_of(shape: PuffShape, p: &LiveParticle) -> Mask {
    match shape {
        PuffShape::Soft => Mask::Soft,
        PuffShape::Puff => Mask::Puff(p.key % PUFFS),
        PuffShape::Ring => Mask::Ring,
        PuffShape::Fire => Mask::Fire((p.t * FIRE_FRAMES) as u8),
    }
}

/// The class ramp at the particle's age; a surface colour replaces the ramp's
/// colour and keeps its alpha, premultiplied.
fn tint_of(p: &LiveParticle, class: &ParticleClass) -> u32 {
    let c = ramp_at(class.ramp, p.t);
    if p.surface == 0 {
        return c;
    }
    let a = c >> 24;
    (c & 0xff00_0000) | scale_rgb(0xff00_0000 | p.surface, a * 256 / 255) & 0x00ff_ffff
}

/// Linear between the ramp's stops, all four channels. An empty ramp (a table
/// mistake) is invisible.
fn ramp_at(ramp: &[u32], t: f32) -> u32 {
    let Some(last) = ramp.len().checked_sub(1) else {
        return 0;
    };
    let at = t.clamp(0.0, 1.0) * last as f32;
    let i = (at as usize).min(last);
    let (a, b) = (ramp[i], ramp[(i + 1).min(last)]);
    let k = ((at - i as f32) * 256.0) as u32;
    let mix = |shift: u32| {
        let (x, y) = ((a >> shift) & 0xff, (b >> shift) & 0xff);
        ((x * (256 - k) + y * k) >> 8) << shift
    };
    mix(24) | mix(16) | mix(8) | mix(0)
}

/// "Over" sprites are drawn far to near. In place, so a frame allocates
/// nothing here.
pub fn sort_back_to_front(over: &mut [SpriteDraw]) {
    over.sort_unstable_by(|a, b| a.inv_z.total_cmp(&b.inv_z));
}

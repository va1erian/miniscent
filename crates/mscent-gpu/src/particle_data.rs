//! A frame's particle draws as the bytes `particles.wgsl` reads, in the order
//! `draw_particles` draws them: "over" sprites back to front, then additive
//! sprites and each pixel of each streak, after the decals. The half-res
//! layer's sprites are drawn at full size, as a band that cannot hold the layer
//! draws them: the layer saves software's time, which a GPU does not need.

use std::ops::Range;

use mscent_render::particles::plan::{decal_plan, sprite_plan, streak_pixels};
use mscent_render::particles::{DecalDraw, ParticleDraws, SpriteDraw};

/// A `Sprite`, a `Decal` and a decal's corner of `particles.wgsl`.
pub(crate) const SPRITE_BYTES: u64 = 48;
pub(crate) const DECAL_BYTES: u64 = 80;
pub(crate) const CORNER_BYTES: u64 = 16;

/// This frame's draws, kept so a frame does not allocate them again.
#[derive(Default)]
pub(crate) struct Listed {
    pub sprites: Vec<u8>,
    pub decals: Vec<u8>,
    pub corners: Vec<u8>,
    /// Instances of `sprites` drawn "over", and those added.
    pub over: Range<u32>,
    pub add: Range<u32>,
}

fn push(out: &mut Vec<u8>, words: &[[u8; 4]]) {
    for w in words {
        out.extend_from_slice(w);
    }
}

fn count(bytes: &[u8], each: u64) -> u32 {
    u32::try_from(bytes.len() as u64 / each).unwrap_or(u32::MAX)
}

impl Listed {
    fn push_sprite(&mut self, s: &SpriteDraw, size: (u32, u32)) {
        let Some(p) = sprite_plan(s, size.0, size.1) else {
            return;
        };
        let f = |v: f32| v.to_le_bytes();
        let u = |v: u32| v.to_le_bytes();
        let pixels = [p.cols.0, p.rows.0, p.cols.1, p.rows.1].map(|v| f(v as f32));
        push(&mut self.sprites, &pixels);
        push(
            &mut self.sprites,
            &[f(p.origin.0), f(p.origin.1), f(p.scale), f(s.inv_z)],
        );
        push(&mut self.sprites, &[u(s.tint), u(p.start), u(p.side), u(0)]);
    }

    /// One pixel of a streak, as a sprite one pixel wide with no mask.
    fn push_pixel(&mut self, (x, y): (u32, u32), inv_z: f32, cover: u8, tint: u32) {
        // `plot` in beam.rs: at or behind the eye passes against nothing, and a
        // NaN draws nothing.
        if inv_z.is_nan() || inv_z <= 0.0 {
            return;
        }
        let f = |v: f32| v.to_le_bytes();
        let u = |v: u32| v.to_le_bytes();
        let pixels = [x, y, x + 1, y + 1].map(|v| f(v as f32));
        push(&mut self.sprites, &pixels);
        push(&mut self.sprites, &[f(0.0), f(0.0), f(0.0), f(inv_z)]);
        push(&mut self.sprites, &[u(tint), u(0), u(0), u(cover.into())]);
    }

    /// A decal's planes, and its polygon as a fan of triangles.
    fn push_decal(&mut self, d: &DecalDraw) {
        let Some(p) = decal_plan(d) else {
            return;
        };
        let index = count(&self.decals, DECAL_BYTES);
        let f = |v: f32| v.to_le_bytes();
        let u = |v: u32| v.to_le_bytes();
        push(
            &mut self.decals,
            &[f(p.origin.0), f(p.origin.1), f(0.0), f(0.0)],
        );
        for plane in [p.base, p.gx, p.gy] {
            push(
                &mut self.decals,
                &[f(plane[0]), f(plane[1]), f(plane[2]), f(0.0)],
            );
        }
        push(
            &mut self.decals,
            &[u(p.start), u(p.side), u(p.darkness), u(0)],
        );
        let points = &d.points[..usize::from(d.len).min(d.points.len())];
        let Some((first, rest)) = points.split_first() else {
            return;
        };
        for pair in rest.windows(2) {
            for c in [first, &pair[0], &pair[1]] {
                push(&mut self.corners, &[f(c.x), f(c.y), u(index), u(0)]);
            }
        }
    }

    /// Replaces the list with `draws` on a `size` frame.
    pub(crate) fn list(&mut self, draws: &ParticleDraws, size: (u32, u32)) {
        self.sprites.clear();
        self.decals.clear();
        self.corners.clear();
        for d in &draws.decals {
            self.push_decal(d);
        }
        for s in draws.layer_over.iter().chain(&draws.over) {
            self.push_sprite(s, size);
        }
        let over_end = count(&self.sprites, SPRITE_BYTES);
        for s in draws.layer_add.iter().chain(&draws.add) {
            self.push_sprite(s, size);
        }
        for st in &draws.streaks {
            streak_pixels(st, size.0, size.1, |x, y, inv_z, cover| {
                self.push_pixel((x, y), inv_z, cover, st.tint);
            });
        }
        self.over = 0..over_end;
        self.add = over_end..count(&self.sprites, SPRITE_BYTES);
    }

    /// Corners of the decals' triangles.
    pub(crate) fn decal_corners(&self) -> u32 {
        count(&self.corners, CORNER_BYTES)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.sprites.is_empty() && self.corners.is_empty()
    }
}

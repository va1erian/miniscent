//! Step 4 of the budget: when the frame cannot afford its big sprites, draw
//! them through the half-res layer (`mscent_render::particles` has the
//! drawing). The cut is computed after, from the cheaper costs.
//!
//! Only Normal and Cosmetic sprites wider than [`BIG_DIAMETER`] move, and only
//! those the scene shows ([`MIN_SEEN`]): a hidden one is cheap where it is and
//! would only widen the layer's region. Essential draws stay crisp. Nothing
//! moves unless the saving on the fill beats what the layer costs for the
//! region those sprites cover, so a frame is never made dearer.
//!
//! The decision is made on the full-size lists every frame, so it follows the
//! scene and not the previous frame's choice. A scene that sits at the edge
//! would then flip between full size and half size on adjacent frames, and
//! half-res smoke beside crisp smoke reads as flicker; so once the layer was
//! needed it stays on for [`HOLD_FRAMES`] frames, counted in frames (not time)
//! so a script replays.

use mscent_render::Framebuffer;
use mscent_render::particles::{ParticleDraws, SpriteDraw};

use super::budget::{
    LAYER_BLEND, LAYER_OVERHEAD, MAX_RADIUS, OVER_WEIGHT, Tag, Tags, clipped_area, layer_cost,
    seen, sprite_cost,
};
use super::class::Tier;

/// Sprites wider than this go to the layer.
pub const BIG_DIAMETER: f32 = 64.0;
/// The least visible fraction of a sprite (of five probes) worth moving.
pub const MIN_SEEN: f32 = 0.4;
/// Frames the layer stays on after the last one that needed it: half a second
/// at 60 fps, about as long as an explosion's smoke takes to thin.
pub const HOLD_FRAMES: u8 = 30;

/// The screen area the layer has to prepare and composite: the union of its
/// sprites' boxes, as one box. A box that is too big only overcharges, and the
/// renderer prepares less (each band does its own part).
#[derive(Clone, Copy, Debug)]
pub(super) struct Region {
    min: (f32, f32),
    max: (f32, f32),
}

impl Region {
    pub(super) const EMPTY: Self = Self {
        min: (f32::MAX, f32::MAX),
        max: (f32::MIN, f32::MIN),
    };

    /// Grow to hold the on-screen part of `s`; a sprite off the screen or with
    /// a hostile size adds nothing.
    pub(super) fn include(&mut self, s: &SpriteDraw, w: f32, h: f32) {
        if clipped_area(s, w, h) <= 0.0 {
            return;
        }
        let r = s.radius.min(MAX_RADIUS);
        self.min = (
            self.min.0.min((s.x - r).max(0.0)),
            self.min.1.min((s.y - r).max(0.0)),
        );
        self.max = (
            self.max.0.max((s.x + r).min(w)),
            self.max.1.max((s.y + r).min(h)),
        );
    }

    pub(super) fn is_empty(&self) -> bool {
        !(self.max.0 > self.min.0 && self.max.1 > self.min.1)
    }

    /// The layer's cost for this region, or 0 when it is empty.
    pub(super) fn cost(&self) -> f32 {
        if self.is_empty() {
            return 0.0;
        }
        LAYER_OVERHEAD + LAYER_BLEND * (self.max.0 - self.min.0) * (self.max.1 - self.min.1)
    }
}

pub(super) fn layer_region(draws: &ParticleDraws, w: f32, h: f32) -> Region {
    let mut region = Region::EMPTY;
    for s in draws.layer_over.iter().chain(&draws.layer_add) {
        region.include(s, w, h);
    }
    region
}

/// What the costs are measured against.
#[derive(Clone, Copy)]
pub struct Screen<'a> {
    /// The last frame's depth, for judging what the scene hides.
    pub depth: Option<&'a Framebuffer>,
    pub width: f32,
    pub height: f32,
}

/// The layer's state from frame to frame: how long it is held on.
#[derive(Clone, Copy, Debug, Default)]
pub struct Gate {
    hold: u8,
}

impl Gate {
    /// Frames the layer will still be held, for tests.
    pub fn held(&self) -> u8 {
        self.hold
    }

    /// Forget the hold, as when the effects are cleared.
    pub fn reset(&mut self) {
        self.hold = 0;
    }

    /// Moves the sprites the layer makes cheaper out of `draws` and `tags`
    /// into their layer lists when `allowed` and the frame is `over` budget
    /// (or was lately), and the saving beats the layer's fixed cost. `true`
    /// when anything moved, and so costs must be taken again. Allocates
    /// nothing while the lists are as large as the fullest frame.
    pub fn apply(
        &mut self,
        (allowed, over): (bool, bool),
        (draws, tags): (&mut ParticleDraws, &mut Tags),
        screen: &Screen,
    ) -> bool {
        let held = self.hold > 0;
        self.hold = self.hold.saturating_sub(1);
        if !allowed || !(over || held) {
            return false;
        }
        let (mut saved, mut region) = (0.0, Region::EMPTY);
        let lists = [
            (&draws.over, &tags.over, OVER_WEIGHT),
            (&draws.add, &tags.add, 1.0),
        ];
        for (list, list_tags, weight) in lists {
            for (s, t) in list.iter().zip(list_tags) {
                let g = gain(s, *t, weight, screen);
                if g > 0.0 {
                    saved += g;
                    region.include(s, screen.width, screen.height);
                }
            }
        }
        if saved <= region.cost() {
            return false;
        }
        move_where(
            (&mut draws.over, &mut tags.over),
            (&mut draws.layer_over, &mut tags.layer_over),
            |s, t| gain(s, *t, OVER_WEIGHT, screen) > 0.0,
        );
        move_where(
            (&mut draws.add, &mut tags.add),
            (&mut draws.layer_add, &mut tags.layer_add),
            |s, t| gain(s, *t, 1.0, screen) > 0.0,
        );
        if over {
            self.hold = HOLD_FRAMES;
        }
        true
    }
}

/// What drawing `s` through the layer saves on its fill, or 0 for a sprite that
/// stays: one that is Essential, small, or mostly hidden.
fn gain(s: &SpriteDraw, tag: Tag, weight: f32, screen: &Screen) -> f32 {
    // A NaN or infinite radius fails this and stays; the renderer refuses it.
    let big = s.radius.is_finite() && 2.0 * s.radius.min(MAX_RADIUS) > BIG_DIAMETER;
    if tag.tier == Tier::Essential || !big || seen(s, screen.depth) < MIN_SEEN {
        return 0.0;
    }
    let (w, h, depth) = (screen.width, screen.height, screen.depth);
    (sprite_cost(s, weight, depth, w, h) - layer_cost(s, weight, depth, w, h)).max(0.0)
}

/// Moves what `moves` picks from one list, with its tags, to the end of
/// another, keeping the order of both.
fn move_where(
    (list, tags): (&mut Vec<SpriteDraw>, &mut Vec<Tag>),
    (to, to_tags): (&mut Vec<SpriteDraw>, &mut Vec<Tag>),
    moves: impl Fn(&SpriteDraw, &Tag) -> bool,
) {
    let n = list.len().min(tags.len());
    let mut kept = 0;
    for i in 0..n {
        let (s, t) = (list[i], tags[i]);
        if moves(&s, &t) {
            to.push(s);
            to_tags.push(t);
        } else {
            list[kept] = s;
            tags[kept] = t;
            kept += 1;
        }
    }
    list.truncate(kept);
    tags.truncate(kept);
}

#[cfg(test)]
#[path = "../fx_layer_tests.rs"]
mod fx_layer_tests;

//! The frame's particle budget. It is counted in pixels rather than time, so a
//! script gives the same picture on any machine at any worker count.
//!
//! One unit is one additive sprite pixel; a "screen" is the frame's pixel count
//! in those units. The weights come from the measured table in
//! `docs/particles.md`, as multiples of the 0.55 ns of that pixel.

use mscent_render::particles::{ParticleDraws, SpriteDraw, StreakDraw};
use mscent_render::shade::mul_rgb;
use mscent_render::{Framebuffer, Quality};

use super::class::{Blend, Tier};

/// 0.68 ns against 0.55 ns per pixel.
const OVER_WEIGHT: f32 = 1.25;
/// About 35 ns per sprite and 20 ns per row, which is what small sprites cost.
const SPRITE_SETUP: f32 = 64.0;
const ROW_SETUP: f32 = 36.0;
/// About 25 ns per streak and 5.7 ns per pixel of its width.
const STREAK_SETUP: f32 = 45.0;
const STREAK_PIXEL: f32 = 10.0;
/// A hidden pixel costs its depth test: 0.08 ns against 0.55 ns.
const OCCLUDED_WEIGHT: f32 = 0.15;
/// Sprites wider than this are worth five depth probes.
const PROBE_DIAMETER: f32 = 32.0;
/// Matches the renderer's cap on a drawn sprite.
const MAX_RADIUS: f32 = 512.0;
/// Keys 0..=255, as `LiveParticle::key` holds.
const KEYS: usize = 256;
/// Keys over which a particle fades out as the cut comes down on it.
const FADE_KEYS: u16 = 32;
/// `keep` when nothing is thinned: every key clears the fade.
const KEEP_ALL: u16 = KEYS as u16 + FADE_KEYS;
/// How far `keep` climbs per frame: a cut applies at once and lifts over about
/// a second, so a spike does not flicker.
const KEEP_RISE: u16 = 5;
const OPAQUE: u16 = 256;

/// What a quality level may spend on particles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Budget, in multiples of the frame's pixels.
    pub screens: f32,
    /// Fraction of Normal and Cosmetic particles spawned at all.
    pub density: f32,
}

impl Limits {
    pub fn of(quality: Quality) -> Self {
        let (screens, density) = match quality {
            Quality::High => (5.0, 1.0),
            Quality::Medium => (3.0, 0.7),
            Quality::Low => (1.5, 0.4),
        };
        Self { screens, density }
    }
}

/// Which particle a draw came from, so it can be thinned.
#[derive(Clone, Copy, Debug)]
pub struct Tag {
    pub tier: Tier,
    pub key: u8,
}

impl Tag {
    pub const ESSENTIAL: Self = Self {
        tier: Tier::Essential,
        key: 0,
    };
}

/// One tag per entry of each list in [`ParticleDraws`]; the lists keep their
/// capacity from frame to frame.
#[derive(Debug, Default)]
pub struct Tags {
    over: Vec<Tag>,
    add: Vec<Tag>,
    streaks: Vec<Tag>,
}

/// Fills the draw lists with their tags, dropping what is off the screen.
pub struct Sink<'a> {
    pub out: &'a mut ParticleDraws,
    pub tags: &'a mut Tags,
    pub width: f32,
    pub height: f32,
}

impl Sink<'_> {
    pub fn sprite(&mut self, s: SpriteDraw, blend: Blend, tag: Tag) {
        if clipped_area(&s, self.width, self.height) <= 0.0 {
            return;
        }
        match blend {
            Blend::Add => {
                self.out.add.push(s);
                self.tags.add.push(tag);
            }
            Blend::Over => {
                self.out.over.push(s);
                self.tags.over.push(tag);
            }
        }
    }

    pub fn streak(&mut self, s: StreakDraw, tag: Tag) {
        self.out.streaks.push(s);
        self.tags.streaks.push(tag);
    }
}

impl Tags {
    pub fn clear(&mut self) {
        self.over.clear();
        self.add.clear();
        self.streaks.clear();
    }
}

/// The current cut per tier: a particle draws while `key < keep`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keep {
    pub normal: u16,
    pub cosmetic: u16,
}

impl Default for Keep {
    fn default() -> Self {
        Self {
            normal: KEEP_ALL,
            cosmetic: KEEP_ALL,
        }
    }
}

impl Keep {
    /// Moves toward `target`: down at once, up slowly.
    pub fn follow(&mut self, target: Keep) {
        let step = |now: u16, to: u16| to.min(now.saturating_add(KEEP_RISE));
        self.normal = step(self.normal, target.normal);
        self.cosmetic = step(self.cosmetic, target.cosmetic);
    }

    fn of(&self, tier: Tier) -> u16 {
        match tier {
            Tier::Essential => KEEP_ALL,
            Tier::Normal => self.normal,
            Tier::Cosmetic => self.cosmetic,
        }
    }
}

/// Cost of the frame by tier and key. On the stack, so a frame allocates
/// nothing here.
pub struct Tally {
    essential: f32,
    normal: [f32; KEYS],
    cosmetic: [f32; KEYS],
}

impl Tally {
    /// Costs every draw. `depth` is the last frame's picture: the scene is not
    /// drawn yet when this runs, and a wrong probe costs a little time for a
    /// frame, never a visible error.
    pub fn of(
        draws: &ParticleDraws,
        tags: &Tags,
        depth: Option<&Framebuffer>,
        w: f32,
        h: f32,
    ) -> Self {
        let mut t = Self {
            essential: 0.0,
            normal: [0.0; KEYS],
            cosmetic: [0.0; KEYS],
        };
        for (s, tag) in draws.over.iter().zip(&tags.over) {
            t.add(*tag, sprite_cost(s, OVER_WEIGHT, depth, w, h));
        }
        for (s, tag) in draws.add.iter().zip(&tags.add) {
            t.add(*tag, sprite_cost(s, 1.0, depth, w, h));
        }
        for (s, tag) in draws.streaks.iter().zip(&tags.streaks) {
            t.add(*tag, streak_cost(s, w, h));
        }
        t
    }

    fn add(&mut self, tag: Tag, cost: f32) {
        let key = usize::from(tag.key);
        match tag.tier {
            Tier::Essential => self.essential += cost,
            Tier::Normal => self.normal[key] += cost,
            Tier::Cosmetic => self.cosmetic[key] += cost,
        }
    }

    /// The cut that fits `budget`. Essential is always drawn; Cosmetic goes
    /// first, then Normal.
    pub fn fit(&self, budget: f32) -> Keep {
        let left = budget - self.essential;
        let normal: f32 = self.normal.iter().sum();
        let cosmetic: f32 = self.cosmetic.iter().sum();
        if normal + cosmetic <= left {
            return Keep::default();
        }
        if normal <= left {
            return Keep {
                normal: KEEP_ALL,
                cosmetic: cut(&self.cosmetic, left - normal),
            };
        }
        Keep {
            normal: cut(&self.normal, left),
            cosmetic: 0,
        }
    }
}

/// The most keys, from 0 up, whose cost fits in `room`.
fn cut(costs: &[f32; KEYS], room: f32) -> u16 {
    let mut sum = 0.0;
    for (key, c) in costs.iter().enumerate() {
        sum += c;
        if sum > room {
            return key as u16;
        }
    }
    KEEP_ALL
}

/// Removes what `keep` cuts and fades what it is about to, in place.
pub fn thin(draws: &mut ParticleDraws, tags: &Tags, keep: Keep) {
    retain_tagged(&mut draws.over, &tags.over, keep, |s, k| {
        s.tint = scale_argb(s.tint, k);
    });
    retain_tagged(&mut draws.add, &tags.add, keep, |s, k| {
        s.tint = mul_rgb(s.tint, u32::from(k));
    });
    retain_tagged(&mut draws.streaks, &tags.streaks, keep, |s, k| {
        s.tint = mul_rgb(s.tint, u32::from(k));
    });
}

fn retain_tagged<T: Copy>(list: &mut Vec<T>, tags: &[Tag], keep: Keep, fade: impl Fn(&mut T, u16)) {
    let mut kept = 0;
    for i in 0..list.len().min(tags.len()) {
        let Some(k) = opacity(tags[i], keep) else {
            continue;
        };
        let mut draw = list[i];
        if k < OPAQUE {
            fade(&mut draw, k);
        }
        list[kept] = draw;
        kept += 1;
    }
    list.truncate(kept);
}

/// `0..=256` of a particle at this cut; `None` when it is gone.
fn opacity(tag: Tag, keep: Keep) -> Option<u16> {
    let above = keep.of(tag.tier).checked_sub(u16::from(tag.key) + 1)?;
    Some(((above + 1).min(FADE_KEYS)) * OPAQUE / FADE_KEYS)
}

/// A premultiplied colour scaled in all four channels, so it stays valid.
fn scale_argb(c: u32, k: u16) -> u32 {
    let k = u32::from(k);
    let a = ((c >> 24) * k) >> 8;
    (a << 24) | (mul_rgb(c, k) & 0x00ff_ffff)
}

/// Total cost of a list, for tests and stats.
pub fn cost_of(draws: &ParticleDraws, depth: Option<&Framebuffer>, w: f32, h: f32) -> f32 {
    let over = draws.over.iter();
    let add = draws.add.iter();
    over.map(|s| sprite_cost(s, OVER_WEIGHT, depth, w, h))
        .sum::<f32>()
        + add.map(|s| sprite_cost(s, 1.0, depth, w, h)).sum::<f32>()
        + draws
            .streaks
            .iter()
            .map(|s| streak_cost(s, w, h))
            .sum::<f32>()
}

fn clipped_box(s: &SpriteDraw, w: f32, h: f32) -> (f32, f32) {
    let r = s.radius.min(MAX_RADIUS);
    let across = (s.x + r).min(w) - (s.x - r).max(0.0);
    let down = (s.y + r).min(h) - (s.y - r).max(0.0);
    (across, down)
}

/// Pixels of a sprite on the screen; NaN and negative sizes come out as 0. The
/// check comes first because `min` and `max` would quietly drop a NaN and
/// charge a sprite the renderer refuses as if it filled the screen.
fn clipped_area(s: &SpriteDraw, w: f32, h: f32) -> f32 {
    if !(s.x.is_finite() && s.y.is_finite() && s.radius.is_finite()) {
        return 0.0;
    }
    let (across, down) = clipped_box(s, w, h);
    if across > 0.0 && down > 0.0 {
        across * down
    } else {
        0.0
    }
}

fn sprite_cost(s: &SpriteDraw, weight: f32, depth: Option<&Framebuffer>, w: f32, h: f32) -> f32 {
    let area = clipped_area(s, w, h);
    if area <= 0.0 {
        return 0.0;
    }
    let seen = depth.map_or(1.0, |fb| visible_fraction(s, fb));
    let pixels = area * weight * (OCCLUDED_WEIGHT + (1.0 - OCCLUDED_WEIGHT) * seen);
    SPRITE_SETUP + ROW_SETUP * clipped_box(s, w, h).1 + pixels
}

/// How much of a large sprite is in front of the scene, from five probes.
fn visible_fraction(s: &SpriteDraw, fb: &Framebuffer) -> f32 {
    if 2.0 * s.radius <= PROBE_DIAMETER {
        return 1.0;
    }
    let d = s.radius.min(MAX_RADIUS) * 0.5;
    let points = [(0.0, 0.0), (-d, 0.0), (d, 0.0), (0.0, -d), (0.0, d)];
    let (mut on_screen, mut seen) = (0u8, 0u8);
    for (dx, dy) in points {
        // Saturating casts: a point off in the distance just misses the buffer.
        if let Some(z) = fb.depth_at((s.x + dx) as i32, (s.y + dy) as i32) {
            on_screen += 1;
            seen += u8::from(s.inv_z >= z);
        }
    }
    if on_screen == 0 {
        return 1.0;
    }
    f32::from(seen) / f32::from(on_screen)
}

fn streak_cost(s: &StreakDraw, w: f32, h: f32) -> f32 {
    let across = s.a.x.max(s.b.x).min(w) - s.a.x.min(s.b.x).max(0.0);
    let down = s.a.y.max(s.b.y).min(h) - s.a.y.min(s.b.y).max(0.0);
    if !(across >= 0.0 && down >= 0.0) {
        return STREAK_SETUP;
    }
    STREAK_SETUP + STREAK_PIXEL * across.max(down) * f32::from(s.width.clamp(1, 5))
}

#[cfg(test)]
#[path = "../fx_budget_tests.rs"]
mod fx_budget_tests;

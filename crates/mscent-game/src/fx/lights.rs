//! The dynamic lights of combat: a few short-lived point lights that fade out
//! on the particle clock, so a pause freezes them and a scripted run replays
//! them exactly.
//!
//! The renderer takes them as a read-only slice ([`Lights::active`]); nothing
//! in the game reads them. The slots, and the bit sets inside each light, are
//! made once, so spawning and fading allocate nothing after the first light
//! placed in a slot.

use mscent_math::Vec3;
use mscent_render::scene::Scene;
use mscent_render::{DynLight, MAX_DYN_LIGHTS};
use mscent_world::Bsp;

use super::{MAX_CATCHUP_TICKS, NS_PER_SEC, TICK_HZ};

/// Longest life a light is given, in seconds; flashes last a fraction of one.
const MAX_LIFE: f32 = 10.0;
/// Halvings of a lift tried before it is given up, when it meets solid.
const LIFT_TRIES: u32 = 3;
/// Spacing of the tests along a lift, and the most of them. `is_solid` rather
/// than `segment_clear`, which allocates and would run on every hit; a wall
/// thinner than the spacing could be missed, and none is that thin.
const LIFT_STEP: f32 = 0.25;
const MAX_LIFT_TESTS: u32 = 64;
/// Longest step of the light clock, the same cap as the particles' so a
/// stall drops its time rather than burning a light out unseen.
const MAX_STEP_NS: u64 = MAX_CATCHUP_TICKS as u64 * NS_PER_SEC / TICK_HZ;

/// A light to spawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightSpec {
    pub pos: Vec3,
    pub radius: f32,
    /// Colour at birth; 1.0 adds as much light as a plain texture colour.
    pub color: Vec3,
    /// Seconds to fade to nothing.
    pub life: f32,
    /// Moved from `pos` by this much if that stays in the open, or by half
    /// and then a quarter of it. A light hugging a wall has a near-zero `N.L`
    /// on that wall, so a flash on a wall is lifted off it.
    pub lift: Vec3,
}

/// A light as a `const` table row: a spot of colour that grows no larger
/// than `radius` and dies in `life`.
#[derive(Clone, Copy, Debug)]
pub struct LightRecipe {
    pub radius: f32,
    pub life: f32,
    pub color: Vec3,
    /// How far off the surface it sits, along the surface normal.
    pub lift: f32,
}

impl LightRecipe {
    /// The light at `pos`, lifted along `normal`.
    pub fn at(&self, pos: Vec3, normal: Vec3) -> LightSpec {
        LightSpec {
            pos,
            radius: self.radius,
            color: self.color,
            life: self.life,
            lift: normal * self.lift,
        }
    }
}

#[derive(Clone, Copy)]
struct Life {
    spec: LightSpec,
    age_ns: u64,
    life_ns: u64,
    /// Whether the light has been given its place in the level yet.
    placed: bool,
}

impl Life {
    const EMPTY: Self = Self {
        spec: LightSpec {
            pos: Vec3::ZERO,
            radius: 0.0,
            color: Vec3::ZERO,
            life: 0.0,
            lift: Vec3::ZERO,
        },
        age_ns: 0,
        life_ns: 1,
        placed: false,
    };

    /// Colour now: the birth colour faded by the square of what is left.
    fn color(&self) -> Vec3 {
        let left = 1.0 - self.age_ns as f32 / self.life_ns as f32;
        self.spec.color * (left * left).max(0.0)
    }

    fn strength(&self) -> f32 {
        let c = self.color();
        c.x + c.y + c.z
    }
}

/// The lights alive, the first `live` of each array.
pub struct Lights {
    lights: Vec<DynLight>,
    life: [Life; MAX_DYN_LIGHTS],
    live: usize,
    limit: usize,
}

impl Default for Lights {
    fn default() -> Self {
        Self::new()
    }
}

impl Lights {
    pub fn new() -> Self {
        Self {
            lights: (0..MAX_DYN_LIGHTS).map(|_| DynLight::new()).collect(),
            life: [Life::EMPTY; MAX_DYN_LIGHTS],
            live: 0,
            limit: MAX_DYN_LIGHTS,
        }
    }

    /// Lights alive.
    pub fn live(&self) -> usize {
        self.live
    }

    pub fn clear(&mut self) {
        self.live = 0;
    }

    /// At most `limit` lights at once (up to [`MAX_DYN_LIGHTS`]); the dimmest
    /// alive give way at once if there are more.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.min(MAX_DYN_LIGHTS);
        while self.live > self.limit {
            self.remove(self.dimmest());
        }
    }

    /// Spawns a light. A spec with a value that is not finite, a radius, life
    /// or colour that is not positive, is dropped. With every slot taken the
    /// dimmest gives way, the new light included.
    pub fn add(&mut self, spec: LightSpec, bsp: Option<&Bsp>) {
        if self.limit == 0 || !usable(&spec) {
            return;
        }
        let spec = LightSpec {
            pos: lifted(&spec, bsp),
            life: spec.life.min(MAX_LIFE),
            ..spec
        };
        let life_ns = ((spec.life * NS_PER_SEC as f32) as u64).max(1);
        let fresh = Life {
            spec,
            age_ns: 0,
            life_ns,
            placed: false,
        };
        let slot = if self.live < self.limit {
            self.live += 1;
            self.live - 1
        } else {
            let victim = self.dimmest();
            if fresh.strength() <= self.life[victim].strength() {
                return;
            }
            victim
        };
        self.life[slot] = fresh;
    }

    /// Ages the lights by `elapsed_ns` of the particle clock and drops those
    /// that have run out.
    pub fn advance(&mut self, elapsed_ns: u64) {
        let step = elapsed_ns.min(MAX_STEP_NS);
        let mut i = 0;
        while i < self.live {
            let l = &mut self.life[i];
            l.age_ns = l.age_ns.saturating_add(step);
            if l.age_ns >= l.life_ns {
                self.remove(i);
                continue;
            }
            if l.placed {
                let color = l.color();
                self.lights[i].set_color(color);
            }
            i += 1;
        }
    }

    /// The lights for this frame, placed in `scene` if they are new. Placing
    /// floods the portals once per light, not per frame.
    pub fn active(&mut self, scene: &Scene) -> &[DynLight] {
        for (light, l) in self.lights.iter_mut().zip(&mut self.life[..self.live]) {
            if !l.placed {
                light.place(scene, l.spec.pos, l.spec.radius, l.color());
                l.placed = true;
            }
        }
        &self.lights[..self.live]
    }

    /// Drops light `i` by swapping the last one into its place, which keeps
    /// the live lights together and the spare slots' storage alive.
    fn remove(&mut self, i: usize) {
        self.live -= 1;
        self.lights.swap(i, self.live);
        self.life.swap(i, self.live);
    }

    /// Index of the dimmest light alive; 0 when there are none.
    fn dimmest(&self) -> usize {
        let strength = |i: &usize| self.life[*i].strength();
        (0..self.live)
            .min_by(|a, b| strength(a).total_cmp(&strength(b)))
            .unwrap_or(0)
    }
}

fn usable(s: &LightSpec) -> bool {
    let c = s.color;
    let finite = s.pos.is_finite() && s.lift.is_finite() && c.is_finite();
    let scalars = s.radius.is_finite() && s.life.is_finite();
    // A light of no colour would only take a slot from one that shows.
    let shows = s.radius > 0.0 && s.life > 0.0 && c.x.min(c.y).min(c.z) >= 0.0 && c != Vec3::ZERO;
    finite && scalars && shows
}

/// `spec.pos` moved by its lift, or by less if that would meet solid: a light
/// must stay on its own side of a wall, whatever is open beyond it.
fn lifted(spec: &LightSpec, bsp: Option<&Bsp>) -> Vec3 {
    let Some(bsp) = bsp else { return spec.pos };
    let mut k = 1.0;
    for _ in 0..LIFT_TRIES {
        let p = spec.pos + spec.lift * k;
        if path_open(bsp, spec.pos, p) {
            return p;
        }
        k *= 0.5;
    }
    spec.pos
}

/// No test from `a` to `b` lands in solid, `b` included.
fn path_open(bsp: &Bsp, a: Vec3, b: Vec3) -> bool {
    let tests = (((b - a).length() / LIFT_STEP).ceil() as u32).clamp(1, MAX_LIFT_TESTS);
    (1..=tests).all(|i| !bsp.is_solid(a.lerp(b, i as f32 / tests as f32).to_f64()))
}

#[cfg(test)]
#[path = "../fx_lights_tests.rs"]
mod fx_lights_tests;

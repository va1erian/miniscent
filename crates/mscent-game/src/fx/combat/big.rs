//! The big effects: a room-clearing blast, with what it does to the view, and
//! a brush shattering. Nothing in the game fires them yet.

use mscent_math::{Aabb, Vec3};
use mscent_render::DynLight;
use mscent_render::dynlight::MAX_RADIUS;
use mscent_render::scene::Scene;
use mscent_world::materials::LIGHTPANEL;

use super::super::EffectArgs;
use super::super::recipes_big::{self as big, BLAST_RADIUS};
use super::super::{MAX_CATCHUP_TICKS, NS_PER_SEC as NS, TICK_HZ};
use super::{CombatFx, args_at};

/// How long the white overlay and the shake last, in seconds.
const WHITE_LIFE: f32 = 0.1;
const SHAKE_LIFE: f32 = 0.5;
/// Strongest overlay, out of 256: bright, but the room still shows through.
const WHITE_PEAK: f32 = 200.0;
/// World units; small, so the camera cannot leave the room it is in.
const SHAKE_PEAK: f32 = 0.15;
/// The view feels a blast out to this many of its radii.
const FELT_RADII: f32 = 3.0;
/// Radians per second of each axis of the shake, chosen not to line up.
const SHAKE_RATES: (f32, f32, f32) = (61.0, 47.0, 53.0);
/// Volume of one batch of a shatter, and the most batches a brush gets, so a
/// huge brush cannot exhaust the pools.
const SHATTER_UNIT: f32 = 8.0;
const MAX_SHATTER_BATCHES: u32 = 8;
const NS_PER_SEC: f32 = 1.0e9;
/// The most one step ages the view: a stall drops its time, as it does for the
/// particles and the lights, so the shake does not end before they do.
const MAX_STEP_NS: u64 = MAX_CATCHUP_TICKS as u64 * NS / TICK_HZ;

/// What a blast does to the view this frame. Cosmetic: the game never sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewKick {
    /// Added to the camera position.
    pub shake: Vec3,
    /// How far to lerp the finished frame toward white, `0..=256`.
    pub white: u32,
}

/// The latest blast, for the view. A flood from it says which leaves can see
/// it, so a blast behind a wall neither shakes nor blinds.
#[derive(Default)]
pub(super) struct BlastView {
    active: bool,
    pos: Vec3,
    radius: f32,
    age_ns: u64,
    reach: DynLight,
    placed: bool,
}

impl BlastView {
    pub(super) fn clear(&mut self) {
        self.active = false;
    }

    pub(super) fn advance(&mut self, elapsed_ns: u64) {
        self.age_ns = self.age_ns.saturating_add(elapsed_ns.min(MAX_STEP_NS));
    }

    fn start(&mut self, pos: Vec3, radius: f32) {
        (self.active, self.pos, self.radius) = (true, pos, radius);
        (self.age_ns, self.placed) = (0, false);
    }

    /// `1..=0` of what the view feels at `eye`; 0 when it cannot see the blast.
    /// The flood is capped, so the falloff ends where it does and not at a
    /// leaf's edge.
    fn felt(&mut self, scene: &Scene, eye: Vec3) -> f32 {
        let reach = (FELT_RADII * self.radius).min(MAX_RADIUS);
        if !self.placed {
            let white = Vec3::new(1.0, 1.0, 1.0);
            self.reach.place(scene, self.pos, reach, white);
            self.placed = true;
        }
        if !self.reach.reaches_leaf(scene.bsp().leaf_at_f32(eye)) {
            return 0.0;
        }
        (1.0 - (eye - self.pos).length() / reach).clamp(0.0, 1.0)
    }
}

/// Fades to zero as the square of what is left of `life`.
fn fading(age: f32, life: f32) -> f32 {
    let left = (1.0 - age / life).max(0.0);
    left * left
}

impl CombatFx {
    /// A blast of `radius` at `pos`, with four lights: one at the centre and
    /// three pushed toward the walls, as far as stays in the open. A bad
    /// position or radius does nothing.
    pub fn blast(&mut self, pos: Vec3, radius: f32, bsp: &mscent_world::Bsp) {
        if !pos.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return;
        }
        let scale = radius / BLAST_RADIUS;
        let args = EffectArgs {
            scale,
            ..args_at(pos, Vec3::ZERO)
        };
        self.fx.spawn(big::BLAST, &args);
        let pushes = [
            Vec3::ZERO,
            Vec3::new(1.0, 0.5, 0.2),
            Vec3::new(-1.0, 0.5, 0.2),
            Vec3::new(0.0, -1.0, 0.2),
        ];
        for push in pushes {
            let mut light = big::BLAST_LIGHT.at(pos, push * scale);
            light.radius *= scale;
            self.lights.add(light, Some(bsp));
        }
        self.blast_view.start(pos, radius);
    }

    /// What the latest blast does to a view from `eye`, on the particle clock.
    pub fn view_kick(&mut self, scene: &Scene, eye: Vec3) -> ViewKick {
        let view = &mut self.blast_view;
        let age = view.age_ns as f32 / NS_PER_SEC;
        if !view.active || age >= SHAKE_LIFE.max(WHITE_LIFE) || !eye.is_finite() {
            return ViewKick::default();
        }
        let felt = view.felt(scene, eye);
        let (x, y, z) = SHAKE_RATES;
        let wobble = Vec3::new((age * x).sin(), (age * y).sin(), (age * z).sin());
        let shake = wobble * (SHAKE_PEAK * felt * fading(age, SHAKE_LIFE));
        ViewKick {
            // Never shake the camera through a wall it is standing against.
            shake: if scene.bsp().is_solid((eye + shake).to_f64()) {
                Vec3::ZERO
            } else {
                shake
            },
            white: (WHITE_PEAK * felt * fading(age, WHITE_LIFE)) as u32,
        }
    }

    /// A brush of `material` and mean colour `tint` breaking. Particles fill
    /// `bounds`, more for a bigger brush up to a limit. An inverted or
    /// non-finite box does nothing.
    pub fn shatter(&mut self, bounds: Aabb, material: u16, tint: u32) {
        let (lo, hi) = (bounds.min, bounds.max);
        let ordered = lo.x <= hi.x && lo.y <= hi.y && lo.z <= hi.z;
        if !lo.is_finite() || !hi.is_finite() || !ordered {
            return;
        }
        let size = hi - lo;
        let batches = (size.x * size.y * size.z / SHATTER_UNIT) as u32;
        let centre = lo + size * 0.5;
        let args = EffectArgs {
            surface: tint & 0x00ff_ffff,
            volume: Some(bounds),
            ..args_at(centre, Vec3::ZERO)
        };
        for _ in 0..batches.clamp(1, MAX_SHATTER_BATCHES) {
            self.fx.spawn(big::shatter_of(material), &args);
        }
        if material == LIGHTPANEL {
            self.lights
                .add(big::PANEL_DYING_LIGHT.at(centre, Vec3::ZERO), None);
        }
    }
}

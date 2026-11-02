//! Emitters: where particles start and how fast, plus the small fixed queue
//! that holds emitters with a delay.
//!
//! Every number here may trace back to a level or a table typo, so each is
//! sanitised before use: non-finite becomes zero, ranges are put in order,
//! and results are clamped to the limits below.

use super::WORLD_LIMIT;
use super::class::ClassId;
use super::pool::Born;
use mscent_math::rng::Rng;
use mscent_math::{Aabb, Vec3};

/// Largest accepted `EffectArgs::scale`.
pub const MAX_SCALE: f32 = 100.0;
/// Particles per emitter per spawn, whatever the table says.
pub const MAX_COUNT: u16 = 1024;
/// Fastest birth speed, in units/s.
pub const MAX_SPEED: f32 = 1.0e6;
/// Life bounds in seconds. The floor is one tick, so a life of 0 is a
/// particle that is born and gone on the next tick rather than a divide.
pub const MIN_LIFE: f32 = 1.0 / 60.0;
pub const MAX_LIFE: f32 = 60.0;
/// Longest accepted emitter delay.
pub const MAX_DELAY: f32 = 10.0;
/// Emitters that can wait for their delay at once. A full queue drops.
pub const QUEUE_CAPACITY: usize = 64;

/// Where particles start, and which way they first move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// All at `pos`, moving along `dir` (any direction if `dir` is zero).
    Point,
    /// Inside a ball of this radius (times scale), moving outward.
    Sphere(f32),
    /// At `pos`, within `half_angle` radians of `dir`.
    Cone { half_angle: f32 },
    /// At `pos`, in the half space `dir` faces (+Y if `dir` is zero).
    Hemisphere,
    /// Along `pos`..`end`, moving along `dir` (any direction if zero).
    Segment,
    /// Inside `EffectArgs::volume`, moving in any direction.
    Volume,
}

/// Where a particle gets its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TintSource {
    Class,
    /// The struck material's mean albedo, `EffectArgs::surface`.
    Surface,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Emitter {
    pub class: ClassId,
    pub count: (u16, u16),
    pub shape: Shape,
    pub speed: (f32, f32),
    pub life: (f32, f32),
    /// Seconds after the trigger.
    pub delay: f32,
    pub tint: TintSource,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectArgs {
    pub pos: Vec3,
    /// Surface normal or shot direction; zero means any.
    pub dir: Vec3,
    /// End of a `Segment`.
    pub end: Vec3,
    pub scale: f32,
    /// Mean albedo `0xRRGGBB`; 0 means "use the class ramp".
    pub surface: u32,
    pub volume: Option<Aabb>,
}

pub type Effect = &'static [Emitter];

/// Arguments that are safe to emit from, or `None`. Anything non-finite or
/// beyond the world limit rejects the whole effect rather than guessing.
pub fn validated(args: &EffectArgs) -> Option<EffectArgs> {
    let ok = |v: Vec3| v.is_finite() && within_world(v);
    let scale_ok = args.scale.is_finite();
    if !(ok(args.pos) && args.dir.is_finite() && ok(args.end) && scale_ok) {
        return None;
    }
    let volume = args.volume.filter(|b| ok(b.min) && ok(b.max));
    Some(EffectArgs {
        scale: args.scale.clamp(0.0, MAX_SCALE),
        volume,
        ..*args
    })
}

pub fn within_world(v: Vec3) -> bool {
    v.x.abs().max(v.y.abs()).max(v.z.abs()) <= WORLD_LIMIT
}

/// An emitter's delay in seconds, sanitised.
pub fn delay_of(e: &Emitter) -> f32 {
    finite_or_zero(e.delay).clamp(0.0, MAX_DELAY)
}

/// How many particles this firing makes.
pub fn count_of(e: &Emitter, rng: &mut Rng) -> u16 {
    let (lo, hi) = (e.count.0.min(e.count.1), e.count.0.max(e.count.1));
    let n = u32::from(lo) + rng.below(u32::from(hi - lo) + 1);
    // `n <= hi <= u16::MAX`, so the conversion cannot fail.
    u16::try_from(n).unwrap_or(u16::MAX).min(MAX_COUNT)
}

/// One particle of `e`. `args` must come from [`validated`].
pub fn born(e: &Emitter, args: &EffectArgs, rng: &mut Rng) -> Born {
    let (offset, dir) = place(e.shape, args, rng);
    let speed = (range(rng, e.speed) * args.scale).clamp(0.0, MAX_SPEED);
    let life = range(rng, e.life).clamp(MIN_LIFE, MAX_LIFE);
    Born {
        pos: clamp_world(args.pos + offset),
        vel: dir * speed,
        life,
        key: rng.below(256) as u8,
        surface: match e.tint {
            TintSource::Class => 0,
            TintSource::Surface => args.surface & 0x00FF_FFFF,
        },
    }
}

fn clamp_world(p: Vec3) -> Vec3 {
    let c = |v: f32| v.clamp(-WORLD_LIMIT, WORLD_LIMIT);
    Vec3::new(c(p.x), c(p.y), c(p.z))
}

fn finite_or_zero(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// Uniform in the (ordered, finite) range; a hostile range collapses to a
/// point instead of producing NaN.
fn range(rng: &mut Rng, (a, b): (f32, f32)) -> f32 {
    const LIMIT: f32 = 1.0e9;
    let (a, b) = (
        finite_or_zero(a).clamp(-LIMIT, LIMIT),
        finite_or_zero(b).clamp(-LIMIT, LIMIT),
    );
    rng.range_f32(a.min(b), a.max(b))
}

/// Offset from `args.pos` and unit direction for one particle.
fn place(shape: Shape, args: &EffectArgs, rng: &mut Rng) -> (Vec3, Vec3) {
    let axis = args.dir.normalized();
    match shape {
        Shape::Point => (Vec3::ZERO, axis.unwrap_or_else(|| unit(rng))),
        Shape::Sphere(r) => {
            let d = unit(rng);
            let radius = finite_or_zero(r).max(0.0) * args.scale;
            // Cube root makes the fill uniform in volume, not bunched at the centre.
            (d * (radius * rng.next_f64().cbrt() as f32), d)
        }
        Shape::Cone { half_angle } => (Vec3::ZERO, cone(axis, half_angle, rng)),
        Shape::Hemisphere => {
            let n = axis.unwrap_or(Vec3::new(0.0, 1.0, 0.0));
            let d = unit(rng);
            (Vec3::ZERO, if d.dot(n) < 0.0 { d * -1.0 } else { d })
        }
        Shape::Segment => (
            (args.end - args.pos) * rng.next_f64() as f32,
            axis.unwrap_or_else(|| unit(rng)),
        ),
        Shape::Volume => match args.volume {
            Some(b) => (
                Vec3::new(
                    rng.range_f32(b.min.x, b.max.x),
                    rng.range_f32(b.min.y, b.max.y),
                    rng.range_f32(b.min.z, b.max.z),
                ) - args.pos,
                unit(rng),
            ),
            None => (Vec3::ZERO, unit(rng)),
        },
    }
}

/// Uniform random unit vector.
fn unit(rng: &mut Rng) -> Vec3 {
    let z = rng.range_f32(-1.0, 1.0);
    let phi = rng.range_f32(0.0, std::f32::consts::TAU);
    let r = (1.0 - z * z).max(0.0).sqrt();
    Vec3::new(r * phi.cos(), r * phi.sin(), z)
}

/// Uniform direction within `half_angle` of `axis`; any direction without one.
fn cone(axis: Option<Vec3>, half_angle: f32, rng: &mut Rng) -> Vec3 {
    let Some(axis) = axis else { return unit(rng) };
    let half = finite_or_zero(half_angle).clamp(0.0, std::f32::consts::PI);
    // Uniform in cos(theta) is uniform over the cap's area.
    let z = rng.range_f32(half.cos(), 1.0);
    let phi = rng.range_f32(0.0, std::f32::consts::TAU);
    let r = (1.0 - z * z).max(0.0).sqrt();
    let helper = if axis.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let u = axis.cross(helper).normalized().unwrap_or(helper);
    let v = axis.cross(u);
    axis * z + u * (r * phi.cos()) + v * (r * phi.sin())
}

/// An emitter waiting out its delay.
#[derive(Clone, Copy, Debug)]
pub struct Pending {
    pub emitter: &'static Emitter,
    pub args: EffectArgs,
    left: f32,
}

/// Fixed slots; nothing here allocates.
pub struct Queue {
    slots: [Option<Pending>; QUEUE_CAPACITY],
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            slots: [None; QUEUE_CAPACITY],
        }
    }
}

impl Queue {
    pub fn clear(&mut self) {
        self.slots = [None; QUEUE_CAPACITY];
    }

    /// Stores an emitter to fire in `delay` seconds; false if the queue is full.
    pub fn push(&mut self, emitter: &'static Emitter, args: EffectArgs, delay: f32) -> bool {
        let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) else {
            return false;
        };
        *slot = Some(Pending {
            emitter,
            args,
            left: delay,
        });
        true
    }

    /// Advances slot `i` by `dt` and takes its emitter if it is now due.
    pub fn take_due(&mut self, i: usize, dt: f32) -> Option<Pending> {
        let p = self.slots.get_mut(i)?.as_mut()?;
        p.left -= dt;
        if p.left <= 0.0 {
            self.slots[i].take()
        } else {
            None
        }
    }

    pub fn waiting(&self) -> usize {
        self.slots.iter().flatten().count()
    }
}

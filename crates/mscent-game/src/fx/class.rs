//! Particle classes: everything particles of one kind share, so a particle
//! itself stays a handful of floats and the step loops stay tight.

/// Blend mode the renderer uses. Local to `fx`; the wiring maps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Add,
    Over,
}

/// Mask a puff sprite is drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PuffShape {
    Soft,
    Puff,
    Ring,
    Fire,
}

/// What a particle is drawn as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prim {
    Spark,
    Puff(PuffShape),
    Shard,
}

/// How much the budget may thin a class. Only `Essential` also changes what
/// a full pool does: it replaces its oldest particle instead of dropping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Essential,
    Normal,
    Cosmetic,
}

/// How a class meets the level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Collide {
    /// Nothing; the depth test hides what passes through a wall.
    None,
    /// One `is_solid` per tick; dies on entry.
    Die,
    /// Reflects off the surface it entered, keeping `keep` of its speed.
    Bounce { keep: f32 },
}

/// Index into the class table given to `Fx::new`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassId(pub u8);

/// A class that sheds smaller particles of another class as it flies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trail {
    pub class: ClassId,
    /// Seconds between shed particles.
    pub interval: f32,
}

/// The shared half of a particle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleClass {
    pub prim: Prim,
    pub blend: Blend,
    pub tier: Tier,
    /// World units/s^2 along -Z (see `GRAVITY_AXIS` in `mod.rs`).
    pub gravity: f32,
    /// Fraction of velocity lost per second.
    pub drag: f32,
    pub collide: Collide,
    /// Premultiplied colour and alpha over life; read by the renderer only.
    pub ramp: &'static [u32],
    /// World radius at birth and at death.
    pub size: (f32, f32),
    /// Sparks: tail length per unit of speed.
    pub stretch: f32,
    pub capacity: u16,
    pub trail: Option<Trail>,
}

/// `ClassId` is a byte, so a table longer than this is truncated.
pub const MAX_CLASSES: usize = 256;

/// Particles alive at once across all classes. Later classes of an oversized
/// table get smaller pools, so memory never depends on the table's claims.
pub const MAX_TOTAL_CAPACITY: usize = 4096;

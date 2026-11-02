//! The particle classes and effect recipes of combat, as `const` tables: no
//! parser and no untrusted input. Starting values from `docs/particles.md`, to
//! be tuned against screenshots.

use super::class::{Blend, ClassId, Collide, ParticleClass, Prim, PuffShape, Tier};
use super::emit::{Effect, Emitter, Shape, TintSource};
use super::lights::LightRecipe;
use mscent_math::Vec3;
use mscent_world::materials::{BRICK, FLOOR, LIGHTPANEL, METAL, ROCK};

pub const HOT_SPARK: ClassId = ClassId(0);
pub const SHORT_SPARK: ClassId = ClassId(1);
pub const COLD_SPARK: ClassId = ClassId(2);
pub const CYAN_SPARK: ClassId = ClassId(3);
pub const CHIP: ClassId = ClassId(4);
pub const DUST: ClassId = ClassId(5);
pub const FLASH: ClassId = ClassId(6);
pub const CYAN_FLASH: ClassId = ClassId(7);
pub const GLOW: ClassId = ClassId(8);
pub const FIRE: ClassId = ClassId(9);
pub const RING: ClassId = ClassId(10);
pub const SMOKE: ClassId = ClassId(11);
/// Sits right in front of the eye, so its world size has to be tiny.
pub const GUN_FLASH: ClassId = ClassId(12);

const HOT_RAMP: &[u32] = &[0xffff_ffff, 0xffff_d080, 0xffff_8020, 0xff60_1000];
const COLD_RAMP: &[u32] = &[0xffff_ffff, 0xffc0_e0ff, 0xff40_6080];
const CYAN_RAMP: &[u32] = &[0xffff_ffff, 0xff80_f0ff, 0xff10_6080];
/// Only the alpha is read when a particle carries a surface colour.
const CHIP_RAMP: &[u32] = &[0xff80_8080, 0xff80_8080, 0xc060_6060];
const DUST_RAMP: &[u32] = &[0x6030_3030, 0x4020_2020, 0x0000_0000];
const FLASH_RAMP: &[u32] = &[0xffff_ffff, 0xffff_b060, 0xff30_1000];
const CYAN_FLASH_RAMP: &[u32] = &[0xffff_ffff, 0xff60_d0ff, 0xff00_2030];
const GLOW_RAMP: &[u32] = &[0xffff_a040, 0xff80_3010, 0xff00_0000];
const FIRE_RAMP: &[u32] = &[0xffff_f0c0, 0xffff_a030, 0xff90_3008, 0xff00_0000];
const RING_RAMP: &[u32] = &[0xffff_e0a0, 0xff80_4010, 0xff00_0000];
const SMOKE_RAMP: &[u32] = &[0x0000_0000, 0x8028_2828, 0x6020_2020, 0x0000_0000];

const fn spark(collide: Collide, ramp: &'static [u32], capacity: u16) -> ParticleClass {
    ParticleClass {
        prim: Prim::Spark,
        blend: Blend::Add,
        tier: Tier::Normal,
        gravity: 6.0,
        drag: 0.6,
        collide,
        ramp,
        size: (0.0, 0.0),
        stretch: 0.02,
        capacity,
        trail: None,
    }
}

const fn puff(
    shape: PuffShape,
    blend: Blend,
    tier: Tier,
    ramp: &'static [u32],
    size: (f32, f32),
    capacity: u16,
) -> ParticleClass {
    ParticleClass {
        prim: Prim::Puff(shape),
        blend,
        tier,
        gravity: 0.0,
        drag: 1.0,
        collide: Collide::None,
        ramp,
        size,
        stretch: 0.0,
        capacity,
        trail: None,
    }
}

/// Indexed by the `ClassId`s above; capacities total 4096 at most.
pub static CLASSES: [ParticleClass; 13] = [
    spark(Collide::Bounce { keep: 0.55 }, HOT_RAMP, 1024),
    spark(Collide::Die, HOT_RAMP, 512),
    spark(Collide::Die, COLD_RAMP, 256),
    spark(Collide::Die, CYAN_RAMP, 256),
    ParticleClass {
        // Drawn as a small sprite until shards get their polygons (M9.6).
        prim: Prim::Shard,
        blend: Blend::Over,
        tier: Tier::Normal,
        gravity: 9.0,
        drag: 0.3,
        collide: Collide::Bounce { keep: 0.35 },
        ramp: CHIP_RAMP,
        size: (0.05, 0.03),
        stretch: 0.0,
        capacity: 384,
        trail: None,
    },
    puff(
        PuffShape::Puff,
        Blend::Over,
        Tier::Cosmetic,
        DUST_RAMP,
        (0.1, 0.45),
        256,
    ),
    puff(
        PuffShape::Soft,
        Blend::Add,
        Tier::Essential,
        FLASH_RAMP,
        (0.35, 0.6),
        64,
    ),
    puff(
        PuffShape::Soft,
        Blend::Add,
        Tier::Essential,
        CYAN_FLASH_RAMP,
        (0.3, 0.5),
        64,
    ),
    puff(
        PuffShape::Soft,
        Blend::Add,
        Tier::Normal,
        GLOW_RAMP,
        (0.14, 0.1),
        64,
    ),
    puff(
        PuffShape::Fire,
        Blend::Add,
        Tier::Normal,
        FIRE_RAMP,
        (0.8, 1.7),
        128,
    ),
    puff(
        PuffShape::Ring,
        Blend::Add,
        Tier::Normal,
        RING_RAMP,
        (0.5, 4.0),
        16,
    ),
    puff(
        PuffShape::Puff,
        Blend::Over,
        Tier::Cosmetic,
        SMOKE_RAMP,
        (0.5, 1.6),
        128,
    ),
    puff(
        PuffShape::Soft,
        Blend::Add,
        Tier::Essential,
        CYAN_FLASH_RAMP,
        (0.05, 0.07),
        16,
    ),
];

const fn em(
    class: ClassId,
    count: (u16, u16),
    shape: Shape,
    speed: (f32, f32),
    life: (f32, f32),
) -> Emitter {
    Emitter {
        class,
        count,
        shape,
        speed,
        life,
        delay: 0.0,
        tint: TintSource::Class,
    }
}

const fn on_surface(e: Emitter) -> Emitter {
    Emitter {
        tint: TintSource::Surface,
        ..e
    }
}

const fn after(delay: f32, e: Emitter) -> Emitter {
    Emitter { delay, ..e }
}

const HEMI: Shape = Shape::Hemisphere;

const fn flash() -> Emitter {
    em(FLASH, (1, 1), Shape::Point, (0.0, 0.0), (0.09, 0.11))
}

const fn chips(n: u16) -> Emitter {
    on_surface(em(CHIP, (n, n), HEMI, (2.0, 6.0), (0.5, 1.1)))
}

const fn dust(n: u16) -> Emitter {
    on_surface(em(DUST, (n, n), HEMI, (0.3, 1.0), (0.5, 1.0)))
}

const fn short_sparks(n: u16) -> Emitter {
    em(SHORT_SPARK, (n, n), HEMI, (3.0, 7.0), (0.12, 0.3))
}

pub static HIT_METAL: Effect = &[
    flash(),
    em(HOT_SPARK, (16, 16), HEMI, (5.0, 11.0), (0.25, 0.6)),
    em(GLOW, (1, 1), Shape::Point, (0.0, 0.0), (0.4, 0.4)),
];
pub static HIT_BRICK: Effect = &[flash(), short_sparks(4), chips(6), dust(4)];
pub static HIT_ROCK: Effect = &[flash(), short_sparks(3), chips(8), dust(5)];
pub static HIT_FLOOR: Effect = &[flash(), short_sparks(8), chips(3), dust(2)];
pub static HIT_GLASS: Effect = &[
    flash(),
    em(COLD_SPARK, (10, 10), HEMI, (4.0, 9.0), (0.15, 0.4)),
    chips(6),
];
/// Levels are untrusted, so a material id this table never heard of still
/// gets a reasonable hit.
pub static HIT_UNKNOWN: Effect = &[flash(), short_sparks(8), dust(2)];

/// What a shot does to a surface of this material.
pub fn surface_hit(material: u16) -> Effect {
    match material {
        METAL => HIT_METAL,
        BRICK => HIT_BRICK,
        ROCK => HIT_ROCK,
        FLOOR => HIT_FLOOR,
        LIGHTPANEL => HIT_GLASS,
        _ => HIT_UNKNOWN,
    }
}

/// A cone around the reflected direction, which `EffectArgs::dir` carries.
pub static HIT_DRONE: Effect = &[
    em(
        CYAN_FLASH,
        (2, 2),
        Shape::Sphere(0.15),
        (0.0, 0.5),
        (0.08, 0.12),
    ),
    em(
        CYAN_SPARK,
        (20, 20),
        Shape::Cone { half_angle: 0.7 },
        (5.0, 12.0),
        (0.15, 0.4),
    ),
    on_surface(em(CHIP, (2, 2), HEMI, (2.0, 5.0), (0.4, 0.8))),
];

pub static MUZZLE_PLAYER: Effect = &[em(
    GUN_FLASH,
    (1, 1),
    Shape::Point,
    (0.0, 0.0),
    (0.04, 0.06),
)];
pub static MUZZLE_ENEMY: Effect = &[em(FLASH, (1, 1), Shape::Point, (0.0, 0.0), (0.05, 0.08))];

pub static DRONE_EXPLOSION: Effect = &[
    em(FLASH, (2, 2), Shape::Sphere(0.3), (0.0, 0.5), (0.14, 0.2)),
    em(FIRE, (6, 6), Shape::Sphere(0.7), (0.4, 1.4), (0.35, 0.6)),
    em(
        HOT_SPARK,
        (30, 30),
        Shape::Sphere(0.3),
        (6.0, 15.0),
        (0.4, 0.9),
    ),
    em(
        SHORT_SPARK,
        (30, 30),
        Shape::Sphere(0.3),
        (6.0, 15.0),
        (0.2, 0.5),
    ),
    on_surface(em(
        CHIP,
        (16, 16),
        Shape::Sphere(0.4),
        (3.0, 8.0),
        (0.7, 1.4),
    )),
    after(0.05, em(RING, (1, 1), Shape::Point, (0.0, 0.0), (0.2, 0.2))),
    after(
        0.1,
        em(SMOKE, (12, 12), Shape::Sphere(0.8), (0.1, 0.5), (1.2, 1.8)),
    ),
];

/// The flash of a drone blowing up, lighting the room it is in.
pub const EXPLOSION_LIGHT: LightRecipe = LightRecipe {
    radius: 12.0,
    life: 0.25,
    color: Vec3::new(3.0, 1.8, 0.9),
    lift: 0.0,
};

/// A bolt striking a wall: a small warm flash on the wall and what is near.
pub const SURFACE_LIGHT: LightRecipe = LightRecipe {
    radius: 4.0,
    life: 0.1,
    color: Vec3::new(1.0, 0.8, 0.5),
    lift: 0.6,
};

/// A bolt striking a drone, which it lights by itself since the mesh has no
/// lightmap to take the flash.
pub const DRONE_HIT_LIGHT: LightRecipe = LightRecipe {
    radius: 4.0,
    life: 0.1,
    color: Vec3::new(0.6, 0.95, 1.1),
    lift: 0.5,
};

//! The two effects nothing in the game fires yet: the room-clearing blast and a
//! shattering brush. Starting values from `docs/particles.md`, for a blast of
//! radius [`BLAST_RADIUS`]; `EffectArgs::scale` spreads them to other radii.

use super::emit::{Effect, Shape};
use super::lights::LightRecipe;
use super::recipes::{
    BIG_FIRE, BIG_RING, BIG_SMOKE, BRUSH_DUST, CHIP, COLD_SPARK, EMBER, FLASH, HOT_SPARK, SHARD,
    SHORT_SPARK, after, em, on_surface,
};
use mscent_math::Vec3;
use mscent_world::materials::{BRICK, FLOOR, LIGHTPANEL, METAL, ROCK};

/// The radius the blast's table is written for.
pub const BLAST_RADIUS: f32 = 12.0;

const fn fire_wave() -> super::emit::Emitter {
    em(BIG_FIRE, (8, 8), Shape::Sphere(3.0), (1.0, 4.0), (0.5, 0.9))
}

const fn smoke_wave() -> super::emit::Emitter {
    em(
        BIG_SMOKE,
        (10, 10),
        Shape::Sphere(10.0),
        (0.2, 1.0),
        (3.0, 4.0),
    )
}

const fn ember_wave() -> super::emit::Emitter {
    em(EMBER, (20, 20), Shape::Sphere(8.0), (0.2, 1.0), (2.0, 3.0))
}

pub static BLAST: Effect = &[
    em(FLASH, (4, 4), Shape::Sphere(2.0), (0.0, 1.0), (0.14, 0.2)),
    fire_wave(),
    after(0.1, fire_wave()),
    em(BIG_RING, (1, 1), Shape::Point, (0.0, 0.0), (0.35, 0.35)),
    em(
        HOT_SPARK,
        (150, 150),
        Shape::Sphere(1.0),
        (15.0, 40.0),
        (0.5, 1.0),
    ),
    em(
        SHORT_SPARK,
        (250, 250),
        Shape::Sphere(1.0),
        (15.0, 40.0),
        (0.3, 0.7),
    ),
    on_surface(em(
        CHIP,
        (64, 64),
        Shape::Sphere(2.0),
        (5.0, 14.0),
        (1.0, 2.5),
    )),
    after(0.15, smoke_wave()),
    after(0.3, smoke_wave()),
    after(0.45, smoke_wave()),
    after(0.5, ember_wave()),
    after(1.0, ember_wave()),
    after(1.5, ember_wave()),
];

/// One of the blast's four lights; `CombatFx::blast` says where each goes.
pub const BLAST_LIGHT: LightRecipe = LightRecipe {
    radius: 2.0 * BLAST_RADIUS,
    life: 0.6,
    color: Vec3::new(3.0, 2.2, 1.4),
    lift: 0.6 * BLAST_RADIUS,
};

const fn shards(n: u16) -> super::emit::Emitter {
    on_surface(em(SHARD, (n, n), Shape::Volume, (1.0, 7.0), (0.8, 1.8)))
}

const fn thick_dust(n: u16) -> super::emit::Emitter {
    on_surface(em(
        BRUSH_DUST,
        (n, n),
        Shape::Volume,
        (0.2, 1.2),
        (0.8, 1.6),
    ))
}

const fn volume_sparks(n: u16) -> super::emit::Emitter {
    em(HOT_SPARK, (n, n), Shape::Volume, (4.0, 12.0), (0.3, 0.8))
}

pub static SHATTER_METAL: Effect = &[volume_sparks(16), shards(10)];
pub static SHATTER_BRICK: Effect = &[shards(16), thick_dust(10)];
pub static SHATTER_ROCK: Effect = &[shards(20), thick_dust(12)];
pub static SHATTER_FLOOR: Effect = &[volume_sparks(4), shards(12), thick_dust(6)];
pub static SHATTER_GLASS: Effect = &[
    em(COLD_SPARK, (12, 12), Shape::Volume, (3.0, 9.0), (0.2, 0.5)),
    shards(16),
];
pub static SHATTER_UNKNOWN: Effect = &[volume_sparks(4), shards(8), thick_dust(4)];

/// What a brush of this material does as it breaks.
pub fn shatter_of(material: u16) -> Effect {
    match material {
        METAL => SHATTER_METAL,
        BRICK => SHATTER_BRICK,
        ROCK => SHATTER_ROCK,
        FLOOR => SHATTER_FLOOR,
        LIGHTPANEL => SHATTER_GLASS,
        _ => SHATTER_UNKNOWN,
    }
}

/// A light panel goes out with one last flash.
pub const PANEL_DYING_LIGHT: LightRecipe = LightRecipe {
    radius: 8.0,
    life: 0.3,
    color: Vec3::new(1.4, 1.5, 1.7),
    lift: 0.0,
};

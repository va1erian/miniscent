//! Rooms with a voxel floor, for the tests of light on voxel surfaces.
//!
//! The room is 24 by 24 by 8. Its floor in `x` from 8 to 20 and `y` from 2 to 14
//! is a `Subtract` volume with no sculpting, so the surface there is exactly the
//! plane the brush floor is, and any difference in how the two look is the
//! light's. A slab stands on the volume part of the floor and casts a shadow
//! edge across it; `rig()` follows that edge.

use mscent_math::{Vec3, Vec3d};
use mscent_world::brush::{Brush, Op};
use mscent_world::materials::FLOOR;
use mscent_world::voxel::{CellSize, Volume};
use mscent_world::{Entity, Level};

use crate::crawl::Rig;

pub fn cuboid(lo: [f64; 3], hi: [f64; 3], op: Op, order: u32) -> Brush {
    let (lo, hi) = (
        Vec3d::new(lo[0], lo[1], lo[2]),
        Vec3d::new(hi[0], hi[1], hi[2]),
    );
    Brush::cuboid(lo, hi, op, order)
        .expect("valid cuboid")
        .with_material(FLOOR)
}

pub fn lamp(x: f32, y: f32, z: f32) -> Entity {
    Entity::Light {
        pos: Vec3::new(x, y, z),
        color: Vec3::new(1.0, 1.0, 1.0),
        radius: 60.0,
        intensity: 1.0,
    }
}

/// The room, with part of its floor as a volume of 0.25 cells.
pub fn room() -> Level {
    let volume =
        Volume::new(Op::Subtract, 1, CellSize::Quarter, [32, 8, -16], [3, 3, 2]).expect("valid");
    Level {
        brushes: vec![cuboid([0.0; 3], [24.0, 24.0, 8.0], Op::Subtract, 0)],
        volumes: vec![volume],
        ..Default::default()
    }
}

/// The room with a lamp at `(6, 12, 6)` and a slab on the volume's floor: its
/// shadow edge runs from the slab's foot at `(12, 8)` away from the lamp.
pub fn shadowed_room() -> Level {
    let mut level = room();
    level
        .brushes
        .push(cuboid([12.0, 8.0, 0.0], [14.0, 16.0, 6.0], Op::Add, 2));
    level.entities.push(lamp(6.0, 12.0, 6.0));
    level
}

/// The shadow edge of [`shadowed_room`] on the voxel floor, seen from the north
/// east.
pub fn rig() -> Rig {
    Rig {
        name: "voxel_floor",
        origin: [12.0, 8.0],
        dir: [0.832_050_3, -0.554_700_2],
        centre: 5.0,
        half: 2.5,
        eye_from: [0.554_700_2, 0.832_050_3],
        dist: 6.0,
        eye_z: 5.0,
    }
}

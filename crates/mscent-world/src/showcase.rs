//! A hand-authored demo level that exercises every renderer feature: four
//! rooms joined by doorways and corridors (with a loop), a pillar casting
//! shadows, a stepped platform and a ramp, light panels, four coloured point
//! lights, two enemies and a goal.
//!
//! Plan view, x east and y north, one unit is about a metre:
//!
//! ```text
//!   D (red, brick) ---- C (green, rock, two pillars, goal)
//!   |                    |
//!   A (warm, pillar) -- B (blue, metal, steps)
//! ```

use crate::LevelBuilder;
use crate::entity::{Entity, Level};
use crate::materials::{BRICK, FLOOR, METAL, ROCK};
use mscent_math::{DPlane, Vec3, Vec3d};

fn light(pos: [f32; 3], color: [f32; 3], radius: f32, intensity: f32) -> Entity {
    Entity::Light {
        pos: Vec3::new(pos[0], pos[1], pos[2]),
        color: Vec3::new(color[0], color[1], color[2]),
        radius,
        intensity,
    }
}

fn at(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// An outward-facing plane through the point `p` with a unit-ish normal.
fn plane(n: [f64; 3], through: [f64; 3]) -> DPlane {
    let n = Vec3d::new(n[0], n[1], n[2]);
    let len = n.length();
    let n = n * (1.0 / len);
    DPlane::new(n, n.dot(Vec3d::new(through[0], through[1], through[2])))
}

fn brushes() -> Vec<crate::Brush> {
    let mut b = LevelBuilder::new();
    // A: the start room, brick walls, a metal pillar between the light and
    // the north east corner.
    b.room([0.0, 0.0, 0.0], [20.0, 20.0, 8.0], (BRICK, FLOOR, ROCK));
    b.panel([6.0, 6.0], 2.0, 8.0);
    b.solid([11.0, 11.0, 0.0], [13.0, 13.0, 8.0], METAL);
    // B: steel hall with a two-step platform.
    b.room([32.0, 0.0, 0.0], [50.0, 20.0, 10.0], (METAL, FLOOR, METAL));
    b.panel([41.0, 10.0], 2.5, 10.0);
    b.solid([38.0, 6.0, 0.0], [46.0, 14.0, 1.5], BRICK);
    b.solid([40.0, 8.0, 0.0], [44.0, 12.0, 3.0], BRICK);
    // C: cave with two pillars and the goal.
    b.room([30.0, 34.0, 0.0], [56.0, 54.0, 12.0], (ROCK, ROCK, ROCK));
    b.panel([43.0, 44.0], 2.0, 12.0);
    b.solid([37.0, 41.0, 0.0], [40.0, 44.0, 12.0], METAL);
    b.solid([47.0, 46.0, 0.0], [50.0, 49.0, 12.0], METAL);
    // D: brick room with a ramp.
    b.room([8.0, 36.0, 0.0], [24.0, 52.0, 8.0], (BRICK, FLOOR, ROCK));
    b.panel([16.0, 44.0], 2.0, 8.0);
    b.solid_planes(
        &[
            plane([0.0, 0.0, -1.0], [0.0, 0.0, 0.0]),
            plane([0.0, -1.0, 0.0], [0.0, 38.0, 0.0]),
            plane([0.0, 1.0, 0.0], [0.0, 46.0, 0.0]),
            plane([1.0, 0.0, 0.0], [14.0, 0.0, 0.0]),
            plane([-3.0, 0.0, 8.0], [9.0, 0.0, 0.0]),
        ],
        FLOOR,
    );
    // Links: A-B, B-C, C-D and D-A (a loop). Corridors overlap the rooms.
    b.carve([19.0, 8.0, 0.0], [33.0, 12.0, 4.0], METAL);
    b.carve([40.0, 19.0, 0.0], [44.0, 35.0, 4.0], METAL);
    b.carve([23.0, 42.0, 0.0], [31.0, 46.0, 4.0], BRICK);
    b.carve([8.0, 19.0, 0.0], [12.0, 37.0, 4.0], BRICK);
    b.finish()
}

fn entities() -> Vec<Entity> {
    vec![
        Entity::PlayerStart {
            pos: at(3.0, 3.0, 2.0),
            yaw: -core::f32::consts::FRAC_PI_4,
        },
        // Warm light west of the pillar so it throws a long shadow north east.
        light([6.0, 6.0, 6.5], [1.0, 0.85, 0.6], 26.0, 1.4),
        light([41.0, 10.0, 8.5], [0.4, 0.55, 1.0], 26.0, 1.4),
        light([43.0, 44.0, 10.0], [0.4, 1.0, 0.5], 30.0, 1.4),
        light([16.0, 44.0, 6.5], [1.0, 0.3, 0.25], 24.0, 1.4),
        // Dim lamps in the four corridors so the links read as lit passages.
        light([26.0, 10.0, 3.3], [0.9, 0.9, 1.0], 13.0, 0.9),
        light([42.0, 27.0, 3.3], [0.7, 0.9, 0.9], 13.0, 0.9),
        light([27.0, 44.0, 3.3], [0.9, 0.8, 0.7], 13.0, 0.9),
        light([10.0, 28.0, 3.3], [1.0, 0.8, 0.7], 13.0, 0.9),
        Entity::Enemy {
            pos: at(36.0, 38.0, 1.0),
            kind: 0,
        },
        Entity::Enemy {
            pos: at(15.0, 12.0, 1.0),
            kind: 0,
        },
        Entity::Goal {
            pos: at(52.0, 51.0, 1.5),
            radius: 1.5,
        },
    ]
}

/// The showcase level. Always valid: see `showcase_tests`.
pub fn showcase_level() -> Level {
    Level {
        brushes: brushes(),
        entities: entities(),
    }
}

#[cfg(test)]
mod showcase_tests {
    use super::*;
    use crate::{Bsp, LeafKind, Portals, compile};

    fn bsp() -> Bsp {
        compile(&showcase_level().brushes).expect("showcase compiles")
    }

    #[test]
    fn every_brush_was_accepted() {
        // 4 rooms of 3 brushes, 4 panels, 3 pillars, 2 steps, a ramp, 4 corridors.
        let n = showcase_level().brushes.len();
        assert_eq!(n, 12 + 4 + 3 + 2 + 1 + 4, "a brush was rejected: {n}");
    }

    #[test]
    fn entities_are_valid_and_in_empty_space() {
        let bsp = bsp();
        for e in showcase_level().entities {
            assert!(e.is_valid());
            let leaf = bsp.leaf_at_f32(e.pos());
            assert_eq!(bsp.leaf_kind(leaf), LeafKind::Empty, "{e:?} is in solid");
        }
    }

    #[test]
    fn all_rooms_are_connected() {
        let bsp = bsp();
        let portals = Portals::build(&bsp);
        let start = bsp.leaf_at_f32(at(3.0, 3.0, 2.0));
        let goal = bsp.leaf_at_f32(at(52.0, 51.0, 1.5));
        assert!(portals.flood_fill(start).contains(&goal));
    }
}

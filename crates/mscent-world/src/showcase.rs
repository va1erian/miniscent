//! A hand-authored demo level that exercises every renderer feature: four
//! rooms joined by doorways and corridors (with a loop), a pillar casting
//! shadows, a stepped platform and a ramp, light panels, four coloured point
//! lights, four projectors (spot lights) aimed at the pillar, the platform, the
//! goal and the ramp, two enemies and a goal.
//!
//! Plan view, x east and y north, one unit is about a metre:
//!
//! ```text
//!   D (red, brick) ---- C (green, rock, two pillars, goal)
//!   |                    |
//!   A (warm, pillar) -- B (blue, metal, steps)
//! ```
//!
//! A few surfaces use non-default [`TexInfo`] on purpose: room A's west wall
//! has bricks three times the size, A's floor tiles are rotated 30 degrees,
//! room D's floor tiles are half a tile shifted and 1.5 times larger and its
//! west wall is rotated 90 degrees (to check the bump lighting follows).

use crate::LevelBuilder;
use crate::brush::{Brush, Op};
use crate::entity::{Entity, Level};
use crate::materials::{BRICK, FLOOR, METAL, ROCK};
use crate::texinfo::TexInfo;
use mscent_math::{DPlane, Vec3, Vec3d};

/// A projector at `pos` aimed at `target`, with half angles `cone` = (inner, outer).
fn projector(
    pos: [f32; 3],
    target: [f32; 3],
    color: [f32; 3],
    (radius, intensity): (f32, f32),
    (inner, outer): (f32, f32),
) -> Entity {
    let d = at(target[0], target[1], target[2]) - at(pos[0], pos[1], pos[2]);
    Entity::Projector {
        pos: at(pos[0], pos[1], pos[2]),
        yaw: d.y.atan2(d.x),
        pitch: d.z.atan2(d.x.hypot(d.y)),
        color: Vec3::new(color[0], color[1], color[2]),
        radius,
        intensity,
        inner,
        outer,
    }
}

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

/// Edits the TexInfo of the plane facing `facing` on the brush with the given
/// op and bounds. A missing brush is skipped (tests assert the edits land).
fn retexture(
    brushes: &mut [Brush],
    (op, lo, hi): (Op, [f32; 3], [f32; 3]),
    facing: [f64; 3],
    edit: impl Fn(&mut TexInfo),
) {
    let near = |a: Vec3, b: [f32; 3]| (a - at(b[0], b[1], b[2])).length() < 1e-3;
    let found = brushes.iter_mut().find(|b| {
        let bounds = b.bounds();
        b.op() == op && near(bounds.min, lo) && near(bounds.max, hi)
    });
    let Some(brush) = found else { return };
    let Some(i) = brush.plane_facing(Vec3d::new(facing[0], facing[1], facing[2])) else {
        return;
    };
    let mut info = brush.texinfos()[i];
    edit(&mut info);
    brush.set_texinfo(i, info);
}

fn texture_examples(brushes: &mut [Brush]) {
    let room_a = (Op::Subtract, [0.0, 0.0, 0.0], [20.0, 20.0, 8.0]);
    let floor_a = (Op::Add, [0.0, 0.0, -1.0], [20.0, 20.0, 0.0]);
    let room_d = (Op::Subtract, [8.0, 36.0, 0.0], [24.0, 52.0, 8.0]);
    let floor_d = (Op::Add, [8.0, 36.0, -1.0], [24.0, 52.0, 0.0]);
    retexture(brushes, room_a, [-1.0, 0.0, 0.0], |t| t.scale_by(3.0, 3.0));
    retexture(brushes, floor_a, [0.0, 0.0, 1.0], |t| t.rotate(30.0));
    retexture(brushes, floor_d, [0.0, 0.0, 1.0], |t| {
        t.scale_by(1.5, 1.5);
        t.shift_by(64.0, 64.0);
    });
    retexture(brushes, room_d, [-1.0, 0.0, 0.0], |t| t.rotate(90.0));
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
    let mut out = b.finish();
    texture_examples(&mut out);
    out
}

fn base_entities() -> Vec<Entity> {
    vec![
        Entity::PlayerStart {
            pos: at(3.0, 3.0, 2.0),
            yaw: -core::f32::consts::FRAC_PI_4,
        },
        // Warm light west of the pillar so it throws a long shadow north east.
        light([6.0, 6.0, 6.5], [1.0, 0.85, 0.6], 32.0, 2.4),
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
            pos: at(17.0, 5.0, 1.0),
            kind: 0,
        },
        Entity::Goal {
            pos: at(52.0, 51.0, 1.5),
            radius: 1.5,
        },
    ]
}

/// The projectors: a cold stage spot from room A's north west corner at the
/// pillar (and its shadow), a white one on the platform in B, a warm narrow one
/// on the goal in C and a red-white one down D's ramp.
fn projectors() -> Vec<Entity> {
    vec![
        projector(
            [2.0, 18.0, 7.5],
            [12.0, 12.0, 0.0],
            [0.7, 0.8, 1.0],
            (30.0, 2.2),
            (0.12, 0.3),
        ),
        projector(
            [41.0, 2.0, 9.5],
            [42.0, 10.0, 3.0],
            [1.0, 1.0, 0.95],
            (22.0, 2.0),
            (0.2, 0.45),
        ),
        projector(
            [48.0, 38.0, 11.0],
            [52.0, 51.0, 1.5],
            [1.0, 0.8, 0.4],
            (24.0, 2.6),
            (0.08, 0.22),
        ),
        projector(
            [10.0, 40.0, 7.5],
            [18.0, 44.0, 0.5],
            [1.0, 0.45, 0.4],
            (20.0, 2.0),
            (0.15, 0.4),
        ),
    ]
}

/// The showcase before it had projectors: the fixture of the tests that hold the
/// renderers to each other and measure shadow edges, which a spot's cone would
/// move. Always valid: see `showcase_tests`.
pub fn showcase_base_level() -> Level {
    Level {
        brushes: brushes(),
        entities: base_entities(),
        ..Default::default()
    }
}

/// The showcase level, with its projectors. Always valid: see `showcase_tests`.
pub fn showcase_level() -> Level {
    Level {
        entities: [base_entities(), projectors()].concat(),
        ..showcase_base_level()
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
    fn texture_examples_were_applied() {
        let level = showcase_level();
        let odd = level
            .brushes
            .iter()
            .flat_map(|b| b.texinfos().iter().zip(b.planes()))
            .filter(|(t, p)| {
                **t != TexInfo {
                    material: t.material,
                    ..TexInfo::default_for_normal(p.normal)
                }
            })
            .count();
        assert_eq!(odd, 4, "four planes carry non-default mappings");
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

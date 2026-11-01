//! A playable generated level: the seeded brush layout of [`crate::gen`]
//! dressed with varied materials, one point light per room, a player start,
//! enemies and a goal.

use crate::brush::{Brush, Op};
use crate::entity::{Entity, Level};
use crate::r#gen::{GenParams, generate, room_centers};
use crate::materials::{BRICK, FLOOR, METAL, ROCK};
use mscent_math::Vec3;
use mscent_math::rng::Rng;

/// Light colours handed out to rooms in turn.
const LIGHT_COLORS: [[f32; 3]; 5] = [
    [1.0, 0.85, 0.6],
    [0.45, 0.6, 1.0],
    [0.5, 1.0, 0.55],
    [1.0, 0.4, 0.3],
    [0.9, 0.6, 1.0],
];
const WALLS: [u16; 3] = [BRICK, METAL, ROCK];

/// Brushes and entities for `seed`. The same inputs give the same level.
pub fn generate_level(seed: u64, params: &GenParams) -> Level {
    let base = generate(seed, params);
    let centres = room_centers(seed, params);
    let rooms = centres.len();
    let mut rng = Rng::new(seed ^ 0x6c65_7665_6c73);
    let mut brushes = Vec::with_capacity(base.len() + rooms * 2);
    let mut slabs = Vec::new();
    for (i, brush) in base.into_iter().enumerate() {
        let b = brush.bounds();
        let is_room = i < rooms && brush.op() == Op::Subtract;
        let material = match (brush.op(), is_room) {
            (Op::Subtract, true) => WALLS[(rng.below(3)) as usize % WALLS.len()],
            (Op::Subtract, false) => METAL,
            (Op::Add, _) => *[METAL, ROCK].get(rng.below(2) as usize).unwrap_or(&METAL),
        };
        brushes.push(brush.with_material(material));
        if is_room {
            slabs.push(((b.min.x, b.min.y, b.max.x, b.max.y), (0.0, b.max.z)));
        }
    }
    for ((x0, y0, x1, y1), (floor, top)) in slabs {
        let (lo, hi) = (
            (f64::from(x0), f64::from(y0)),
            (f64::from(x1), f64::from(y1)),
        );
        let mut add = |min: [f64; 3], max: [f64; 3], m: u16| {
            let v = |p: [f64; 3]| mscent_math::Vec3d::new(p[0], p[1], p[2]);
            if let Ok(b) = Brush::cuboid(
                v(min),
                v(max),
                Op::Add,
                u32::try_from(brushes.len()).unwrap_or(u32::MAX),
            ) {
                brushes.push(b.with_material(m));
            }
        };
        add([lo.0, lo.1, floor - 1.0], [hi.0, hi.1, floor], FLOOR);
        let (cx, cy) = ((lo.0 + hi.0) * 0.5, (lo.1 + hi.1) * 0.5);
        let t = f64::from(top);
        add(
            [cx - 3.0, cy - 3.0, t],
            [cx + 3.0, cy + 3.0, t + 1.0],
            crate::materials::LIGHTPANEL,
        );
    }
    Level {
        brushes,
        entities: entities(seed, params, &centres, &mut rng),
    }
}

fn entities(
    seed: u64,
    params: &GenParams,
    centres: &[mscent_math::Vec3d],
    rng: &mut Rng,
) -> Vec<Entity> {
    let mut out = Vec::new();
    let heights = room_heights(seed, params);
    for (i, c) in centres.iter().enumerate() {
        let (x, y) = (c.x as f32, c.y as f32);
        let h = heights.get(i).copied().unwrap_or(8.0);
        let col = LIGHT_COLORS[i % LIGHT_COLORS.len()];
        out.push(Entity::Light {
            pos: Vec3::new(x, y, h - 1.5),
            color: Vec3::new(col[0], col[1], col[2]),
            radius: 34.0,
            intensity: 1.6,
        });
        if i == 0 {
            out.push(Entity::PlayerStart {
                pos: Vec3::new(x - 4.0, y - 4.0, 2.0),
                yaw: -0.8,
            });
        } else if i + 1 == centres.len() {
            out.push(Entity::Goal {
                pos: Vec3::new(x, y, 1.5),
                radius: 1.5,
            });
        } else if rng.below(100) < 70 {
            out.push(Entity::Enemy {
                pos: Vec3::new(x + 3.0, y - 2.0, 1.0),
                kind: 0,
            });
        }
    }
    out
}

/// Ceiling height of each room, read back from the generated brushes.
fn room_heights(seed: u64, params: &GenParams) -> Vec<f32> {
    let rooms = room_centers(seed, params).len();
    generate(seed, params)
        .iter()
        .take(rooms)
        .map(|b| b.bounds().max.z)
        .collect()
}

#[cfg(test)]
mod gen_level_tests {
    use super::*;
    use crate::{LeafKind, compile};

    #[test]
    fn seed_7_level_compiles_with_valid_entities_in_empty_space() {
        let level = generate_level(7, &GenParams::default());
        let bsp = compile(&level.brushes).expect("compiles");
        for e in &level.entities {
            assert!(e.is_valid());
            assert_eq!(
                bsp.leaf_kind(bsp.leaf_at_f32(e.pos())),
                LeafKind::Empty,
                "{e:?}"
            );
        }
        assert!(
            level
                .entities
                .iter()
                .any(|e| matches!(e, Entity::Goal { .. }))
        );
        assert!(
            level
                .entities
                .iter()
                .any(|e| matches!(e, Entity::PlayerStart { .. }))
        );
    }

    #[test]
    fn same_seed_same_level() {
        let p = GenParams::default();
        assert_eq!(generate_level(3, &p), generate_level(3, &p));
    }
}

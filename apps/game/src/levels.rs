//! Built-in levels. The showcase comes from `mscent-world`; the corridor is
//! a small scripted-playthrough level: start room, a straight corridor with
//! an enemy waiting at the far end, and the goal behind it.

use mscent_math::Vec3;
use mscent_world::materials::{BRICK, FLOOR, METAL, ROCK};
use mscent_world::{Entity, Level, LevelBuilder};

fn at(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn light(pos: Vec3, color: [f32; 3], radius: f32) -> Entity {
    Entity::Light {
        pos,
        color: at(color[0], color[1], color[2]),
        radius,
        intensity: 1.4,
    }
}

/// Start room (south), a 28 unit corridor along +Y, an arena (north) with
/// one enemy and the goal. A player who holds W from the start flies the
/// whole way, which is what the scripted tests do.
pub fn corridor_level() -> Level {
    let mut b = LevelBuilder::new();
    b.room([0.0, 0.0, 0.0], [16.0, 16.0, 6.0], (BRICK, FLOOR, ROCK));
    b.panel([8.0, 8.0], 1.5, 6.0);
    b.room([-4.0, 44.0, 0.0], [20.0, 68.0, 8.0], (METAL, FLOOR, METAL));
    b.panel([8.0, 56.0], 2.0, 8.0);
    b.carve([6.0, 14.0, 0.0], [10.0, 46.0, 4.0], BRICK);
    Level {
        brushes: b.finish(),
        entities: vec![
            Entity::PlayerStart {
                pos: at(8.0, 3.0, 2.0),
                yaw: 0.0,
            },
            light(at(8.0, 8.0, 5.0), [1.0, 0.85, 0.6], 22.0),
            light(at(8.0, 20.0, 3.3), [0.9, 0.9, 1.0], 14.0),
            light(at(8.0, 32.0, 3.3), [0.8, 1.0, 0.9], 14.0),
            light(at(8.0, 56.0, 7.0), [0.5, 0.6, 1.0], 28.0),
            Entity::Enemy {
                pos: at(8.0, 52.0, 2.0),
                kind: 0,
            },
            Entity::Goal {
                pos: at(8.0, 64.0, 2.0),
                radius: 1.5,
            },
        ],
        ..Default::default()
    }
}

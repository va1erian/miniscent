//! Level builders shared by the game tests.

use crate::{Arena, Game, Input};
use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_world::{Brush, Entity, Level, Op};

pub fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn vd(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

/// Empty cube room `[-20, 20]^3` in solid rock.
fn room() -> Brush {
    Brush::cuboid(
        vd(-20.0, -20.0, -20.0),
        vd(20.0, 20.0, 20.0),
        Op::Subtract,
        0,
    )
    .unwrap()
}

/// A slab at `x` in `[-1, 1]` splitting the room in two, so nothing sees
/// across it.
fn divider() -> Brush {
    Brush::cuboid(vd(-1.0, -21.0, -21.0), vd(1.0, 21.0, 21.0), Op::Add, 1).unwrap()
}

pub fn open_level(entities: Vec<Entity>) -> Level {
    Level {
        brushes: vec![room()],
        entities,
        ..Default::default()
    }
}

pub fn split_level(entities: Vec<Entity>) -> Level {
    Level {
        brushes: vec![room(), divider()],
        entities,
        ..Default::default()
    }
}

pub fn start(pos: Vec3, yaw: f32) -> Entity {
    Entity::PlayerStart { pos, yaw }
}

pub fn goal(pos: Vec3) -> Entity {
    Entity::Goal { pos, radius: 2.0 }
}

pub fn enemy(pos: Vec3) -> Entity {
    Entity::Enemy { pos, kind: 0 }
}

/// Yaw that makes the player look along +X.
pub const FACE_PLUS_X: f32 = -std::f32::consts::FRAC_PI_2;

/// A far-away goal so levels that are not about winning keep playing.
pub fn far_goal() -> Entity {
    goal(v(-15.0, -15.0, -15.0))
}

pub fn game(level: &Level) -> Game {
    Game::new(Arena::from_level(level).unwrap())
}

/// A reproducible pseudo-random input trace using every axis.
pub fn random_trace(seed: u64, len: usize) -> Vec<Input> {
    let mut rng = Rng::new(seed);
    let mut axis = move || rng.range_f32(-1.0, 1.0);
    (0..len)
        .map(|_| Input {
            thrust: v(axis(), axis(), axis()),
            pitch: axis(),
            yaw: axis(),
            roll: axis(),
            fire: false,
            use_key: false,
            board: false,
        })
        .collect()
}

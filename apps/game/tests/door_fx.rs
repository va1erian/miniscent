//! Doors and combat effects in one session: drawing goes through the effects
//! path, so that path has to carry where the movers are.

use miniscent_game::{GameSession, SessionConfig, assets};
use mscent_collide::Pose;
use mscent_game::{Arena, Game, Input};
use mscent_math::{Vec3, Vec3d};
use mscent_render::{BuildConfig, LevelRenderer, Quality, View};
use mscent_world::{Brush, Entity, Level, MoverGroup, Op};

/// A lit room with the player looking along +X at a door across the room.
fn door_level() -> Level {
    let d = Vec3d::new;
    let cuboid = |a, b, op, order| Brush::cuboid(a, b, op, order).expect("a brush");
    Level {
        brushes: vec![
            cuboid(d(-20.0, -20.0, -20.0), d(20.0, 20.0, 20.0), Op::Subtract, 0),
            cuboid(d(5.0, -8.0, -8.0), d(6.0, 8.0, 8.0), Op::Add, 99),
        ],
        entities: vec![
            Entity::PlayerStart {
                pos: Vec3::new(-5.0, 0.0, 0.0),
                yaw: -std::f32::consts::FRAC_PI_2,
            },
            Entity::Goal {
                pos: Vec3::new(18.0, 18.0, 18.0),
                radius: 0.5,
            },
            Entity::Light {
                pos: Vec3::new(-5.0, 0.0, 5.0),
                color: Vec3::new(1.0, 1.0, 1.0),
                radius: 40.0,
                intensity: 1.0,
            },
        ],
        mover_groups: vec![MoverGroup {
            name: "door".to_string(),
            tags: Vec::new(),
            brushes: vec![1],
        }],
        ..Level::default()
    }
}

/// The picture of the level, with the door moved far off when `opened`.
fn picture(opened: bool) -> Vec<u32> {
    let level = door_level();
    let build = BuildConfig {
        luxel_size: 2.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    let renderer = LevelRenderer::from_level(&level, materials, &build).expect("a level");
    let mut game = Game::new(Arena::from_level(&level).expect("an arena"));
    game.player.orientation =
        View::from_angles(Vec3::ZERO, game.arena.player_yaw, 0.0, 0.0).orientation;
    if opened {
        let door = game.mover_index("door").expect("a door");
        let away = Pose::translated(Vec3d::new(0.0, 0.0, 30.0));
        game.define_pose(door, "open", away).expect("a pose");
        game.move_mover(door, "open", 0.0).expect("a move");
    }
    for _ in 0..2 {
        game.step(&Input::default());
    }
    let cfg = SessionConfig {
        quality: Quality::Medium,
        ..SessionConfig::default()
    };
    let mut session = GameSession::with_game(game, renderer, &cfg);
    let frame = session.render(160, 90).expect("a frame");
    frame.pixels().to_vec()
}

#[test]
fn a_session_draws_a_door_where_it_is_now() {
    let (closed, open) = (picture(false), picture(true));
    let changed = closed.iter().zip(&open).filter(|(a, b)| a != b).count();
    assert!(
        changed > closed.len() / 4,
        "only {changed} of {} pixels moved with the door",
        closed.len()
    );
}

//! Voxel volumes in the game (M11.6): the arena builds their surfaces and
//! every sweep, spawn check and replay goes through them.

use crate::test_util::{
    FACE_PLUS_X, enemy, far_goal, game, goal, open_level, random_trace, start, v,
};
use crate::{Arena, Input, LoadError};
use mscent_math::Vec3d;
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume};
use mscent_world::{Level, Op};

/// The open room with a ball of voxel rock, radius 3, centred at (9, 0, 0).
fn level_with_rock(entities: Vec<mscent_world::Entity>) -> Level {
    let mut level = open_level(entities);
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Half, [10, -8, -8], [1, 1, 1]).expect("valid");
    let mut ctx = Context::new(&volume, &level.brushes);
    let ball = Prim::sphere(Vec3d::new(9.0, 0.0, 0.0), 3.0).expect("valid");
    mscent_world::voxel::apply_dab(&mut volume, &mut ctx, &ball, DabOp::Add { material: 1 });
    level.volumes.push(volume);
    level
}

fn rock_entities() -> Vec<mscent_world::Entity> {
    vec![
        start(v(-15.0, 0.0, 0.0), FACE_PLUS_X),
        goal(v(18.0, 0.0, 0.0)),
    ]
}

fn forward(n: usize) -> Vec<Input> {
    let go = Input {
        thrust: v(0.0, 0.0, 1.0),
        ..Input::default()
    };
    vec![go; n]
}

#[test]
fn the_arena_holds_the_surfaces_of_its_volumes() {
    let arena = Arena::from_level(&level_with_rock(rock_entities())).unwrap();
    assert!(!arena.voxels.chunks.is_empty());
    let inside = Vec3d::new(9.0, 0.0, 0.0);
    assert!(
        !arena.bsp.is_solid(inside),
        "the BSP alone sees an open room"
    );
    assert!(arena.is_solid(inside), "the arena sees the rock");
    assert!(!arena.is_solid(Vec3d::new(0.0, 0.0, 0.0)));
    let plain = Arena::from_level(&open_level(rock_entities())).unwrap();
    assert!(plain.voxels.chunks.is_empty());
}

#[test]
fn flying_into_voxel_rock_stops_where_open_air_does_not() {
    let mut blocked = game(&level_with_rock(rock_entities()));
    let mut free = game(&open_level(rock_entities()));
    blocked.run_trace(&forward(600));
    free.run_trace(&forward(600));
    let x = blocked.player.pos().x;
    assert!(
        x > 4.0 && x < 6.5,
        "stopped at the ball, radius 3 at x 9: {x}"
    );
    assert!(free.player.pos().x > 10.0, "{}", free.player.pos().x);
}

#[test]
fn nothing_spawns_inside_voxel_rock() {
    let inside = v(9.0, 0.0, 0.0);
    let player = vec![start(inside, 0.0), far_goal()];
    let e = Arena::from_level(&level_with_rock(player)).unwrap_err();
    assert_eq!(e, LoadError::SpawnInSolid(0));
    let monster = vec![start(v(-15.0, 0.0, 0.0), 0.0), far_goal(), enemy(inside)];
    let e = Arena::from_level(&level_with_rock(monster)).unwrap_err();
    assert_eq!(e, LoadError::SpawnInSolid(2));
}

#[test]
fn a_replay_through_voxel_rock_is_bit_exact() {
    let spawn = vec![start(v(0.0, 0.0, 0.0), FACE_PLUS_X), far_goal()];
    let level = level_with_rock(spawn.clone());
    // Into the rock, then wandering along it.
    let mut trace = forward(500);
    trace.extend(random_trace(41, 1500));
    let run = || {
        let mut g = game(&level);
        g.run_trace(&trace);
        let p = g.player.pos();
        [p.x, p.y, p.z].map(f32::to_bits)
    };
    assert_eq!(run(), run());
    let plain = {
        let mut g = game(&open_level(spawn));
        g.run_trace(&trace);
        let p = g.player.pos();
        [p.x, p.y, p.z].map(f32::to_bits)
    };
    assert_ne!(run(), plain, "the rock changed where the player went");
}

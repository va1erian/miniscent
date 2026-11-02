//! Shooting destructible rock, read back through collision and replay.

use mscent_math::{Vec3, Vec3d};
use mscent_world::materials::{METAL, ROCK};
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume, apply_dab};
use mscent_world::{Level, Op};

use crate::destruct::{MESHES_PER_TICK, VoxelUpdate};
use crate::test_util::{FACE_PLUS_X, far_goal, game, open_level, start, v};
use crate::{Game, Input};

/// A wall of voxel `material` across the room ahead of the player, at x = 2..4.
fn level_with_wall(material: u16) -> Level {
    let mut level = open_level(vec![start(v(-3.0, 0.0, 0.0), FACE_PLUS_X), far_goal()]);
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Quarter, [0, -16, -16], [2, 2, 2]).expect("valid");
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &level.brushes);
    let wall = Prim::cuboid(Vec3d::new(2.0, -8.0, -8.0), Vec3d::new(4.0, 8.0, 8.0)).expect("valid");
    let m = DabOp::Add {
        material: material as u8,
    };
    apply_dab(&mut volume, &mut ctx, &wall, m);
    level.volumes.push(volume);
    level
}

fn fire() -> Input {
    Input {
        fire: true,
        ..Input::default()
    }
}

fn wall_at(game: &Game, y: f32) -> f32 {
    let from = Vec3::new(-3.0, y, 0.0);
    let t = crate::combat::ray(game.arena.solids(&[]), from, Vec3::new(9.0, y, 0.0));
    if t.hit { -3.0 + t.fraction * 12.0 } else { 9.0 }
}

fn idle(game: &mut Game, steps: usize) {
    for _ in 0..steps {
        game.step(&Input::default());
    }
}

/// Fires once and lets the dab land.
fn shoot(game: &mut Game) {
    game.step(&fire());
    idle(game, 12);
}

#[test]
fn a_shot_carves_destructible_rock_and_collision_follows() {
    let mut game = game(&level_with_wall(ROCK));
    let before = wall_at(&game, 0.0);
    assert!((before - 2.0).abs() < 0.2, "the wall is at {before}");
    shoot(&mut game);
    let after = wall_at(&game, 0.0);
    assert!(after > before + 0.3, "{before} then {after}");
    let aside = wall_at(&game, 3.0);
    assert!((aside - before).abs() < 0.01, "away from the hit: {aside}");
}

#[test]
fn a_shot_leaves_other_materials_alone() {
    let mut game = game(&level_with_wall(METAL));
    let before = game.arena.voxels.clone();
    shoot(&mut game);
    assert_eq!(game.arena.voxels, before);
    assert!(game.take_voxel_updates().is_empty());
}

#[test]
fn a_shot_at_a_brush_wall_carves_nothing() {
    let level = open_level(vec![start(v(0.0, 0.0, 0.0), FACE_PLUS_X), far_goal()]);
    let mut game = game(&level);
    shoot(&mut game);
    assert!(game.take_voxel_updates().is_empty());
}

#[test]
fn meshing_keeps_to_its_budget_and_the_swap_is_whole() {
    let mut game = game(&level_with_wall(ROCK));
    let before = game.arena.voxels.clone();
    game.step(&fire());
    let mut swapped = 0;
    let mut most = 0;
    for _ in 0..16 {
        most = most.max(game.destruct.meshed_last_tick());
        for update in game.take_voxel_updates() {
            let VoxelUpdate::Chunks(list) = update else {
                panic!("not a reset");
            };
            assert!(!list.is_empty());
            swapped += 1;
        }
        // Nothing changes until the tick it all changes on.
        if swapped == 0 {
            assert_eq!(game.arena.voxels, before);
        }
        game.step(&Input::default());
    }
    assert_eq!(swapped, 1, "one dab, one swap");
    assert!(most <= MESHES_PER_TICK, "{most} meshes in a tick");
    assert_ne!(game.arena.voxels, before);
}

#[test]
fn a_replay_with_destruction_is_bit_exact() {
    let level = level_with_wall(ROCK);
    let mut trace = Vec::new();
    for i in 0..600 {
        trace.push(Input {
            fire: i % 30 == 0,
            thrust: v(0.0, 0.0, if i < 300 { 1.0 } else { -1.0 }),
            ..Input::default()
        });
    }
    let run = || {
        let mut g = game(&level);
        g.run_trace(&trace);
        g
    };
    let (a, b) = (run(), run());
    assert_eq!(a.player.pos().x.to_bits(), b.player.pos().x.to_bits());
    assert_eq!(a.player.pos().y.to_bits(), b.player.pos().y.to_bits());
    assert_eq!(a.player.pos().z.to_bits(), b.player.pos().z.to_bits());
    assert_eq!(a.arena.voxels, b.arena.voxels);
    assert_eq!(a.destruct, b.destruct);
}

#[test]
fn a_restart_puts_the_rock_back_and_tells_the_renderer() {
    let mut g = game(&level_with_wall(ROCK));
    let before = g.arena.voxels.clone();
    shoot(&mut g);
    assert_ne!(g.arena.voxels, before);
    g.take_voxel_updates();
    g.restart();
    assert_eq!(g.arena.voxels, before);
    let updates = g.take_voxel_updates();
    assert!(matches!(updates.as_slice(), [VoxelUpdate::Reset(_)]));
    shoot(&mut g);
    assert_ne!(g.arena.voxels, before, "and it can be shot again");
}

#[test]
fn repeated_shots_dig_on_and_never_open_the_box() {
    let mut g = game(&level_with_wall(ROCK));
    let mut last = wall_at(&g, 0.0);
    let mut deeper = 0;
    for _ in 0..12 {
        for _ in 0..80 {
            g.step(&fire());
        }
        idle(&mut g, 40);
        let now = wall_at(&g, 0.0);
        assert!(now >= last - 0.01, "the wall came back: {last} then {now}");
        if now > last + 0.05 {
            deeper += 1;
        }
        last = now;
    }
    assert!(deeper >= 1, "it dug at all");
    assert!(g.destruct.is_idle());
}

#[test]
fn a_full_queue_drops_new_hits_rather_than_fall_behind() {
    let mut g = game(&level_with_wall(ROCK));
    for _ in 0..2000 {
        g.step(&fire());
    }
    idle(&mut g, 200);
    assert!(g.destruct.is_idle());
}

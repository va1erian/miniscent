//! Particles against voxel rock: a ball of Add rock, radius 3, centred at
//! (9, 0, 0) in the open `[-20, 20]^3` room, which the BSP alone cannot see.

use super::fx_tests::{args_at, class, emitter};
use super::*;
use crate::Arena;
use crate::fx::collide::{VOXEL_SWEEP_COST, VOXEL_TEST_COST};
use crate::fx::combat::CombatFx;
use crate::test_util::{FACE_PLUS_X, far_goal, open_level, start, v};
use mscent_math::Vec3d;
use mscent_render::MaterialTable;
use mscent_render::scene::Scene;
use mscent_world::voxel::{CellSize, Context, DabOp, Prim as VPrim, Volume, VoxelWorld};
use mscent_world::{Level, Op, Portals, compile, extract_faces};

const ROCK: Vec3d = Vec3d::new(9.0, 0.0, 0.0);

fn rock_level() -> Level {
    let mut level = open_level(vec![start(v(-15.0, 0.0, 0.0), FACE_PLUS_X), far_goal()]);
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Half, [10, -8, -8], [1, 1, 1]).expect("valid");
    let mut ctx = Context::new(&volume, &level.brushes);
    let ball = VPrim::sphere(ROCK, 3.0).expect("valid");
    mscent_world::voxel::apply_dab(&mut volume, &mut ctx, &ball, DabOp::Add { material: 1 });
    level.volumes.push(volume);
    level
}

const TABLE: [ParticleClass; 3] = [
    class(Collide::None, 1000, Tier::Normal),
    class(Collide::Die, 1000, Tier::Normal),
    class(Collide::Bounce { keep: 0.5 }, 1000, Tier::Normal),
];

/// One spark of `class` at `from`, flying toward +x at 1 m per tick.
fn spark(fx: &mut Fx, class: u8, from: Vec3) {
    let e: &'static [Emitter] = Box::leak(Box::new([emitter(class, 1, Shape::Point, 60.0, 10.0)]));
    let mut a = args_at(from);
    a.dir = v(1.0, 0.0, 0.0);
    fx.spawn(e, &a);
}

#[test]
fn a_spark_that_dies_on_contact_dies_at_voxel_rock_and_not_at_the_bsp_alone() {
    let arena = Arena::from_level(&rock_level()).unwrap();
    let (mut with, mut without) = (Fx::new(&TABLE, 1), Fx::new(&TABLE, 1));
    spark(&mut with, 1, v(0.0, 0.5, 0.3));
    spark(&mut without, 1, v(0.0, 0.5, 0.3));
    for _ in 0..10 {
        with.tick(arena.fx_world());
        without.tick(&arena.bsp);
    }
    assert_eq!(with.live(), 0, "died in the rock");
    assert_eq!(without.live(), 1, "the BSP alone flies through it");
}

#[test]
fn a_bouncing_spark_turns_back_at_voxel_rock_and_never_ends_a_tick_inside_it() {
    let arena = Arena::from_level(&rock_level()).unwrap();
    let mut fx = Fx::new(&TABLE, 1);
    spark(&mut fx, 2, v(0.0, 0.5, 0.3));
    for tick in 0..12 {
        fx.tick(arena.fx_world());
        assert_eq!(fx.live(), 1, "tick {tick}");
        let (p, vel) = (fx.pools[2].pos(0), fx.pools[2].vel(0));
        assert!(!arena.is_solid(p.to_f64()), "tick {tick}: in rock at {p:?}");
        if tick >= 8 {
            assert!(vel.x < -25.0, "tick {tick}: turned back, {vel:?}");
        }
    }
    let p = fx.pools[2].pos(0);
    assert!(p.x < 6.5, "stayed on the near side of the ball: {p:?}");
}

#[test]
fn a_level_without_volumes_costs_and_behaves_as_the_bsp_alone() {
    let arena = Arena::from_level(&open_level(vec![
        start(v(0.0, 0.0, 0.0), FACE_PLUS_X),
        far_goal(),
    ]))
    .unwrap();
    assert!(arena.voxels.volumes.is_empty());
    let (mut a, mut b) = (Fx::new(&TABLE, 3), Fx::new(&TABLE, 3));
    for fx in [&mut a, &mut b] {
        spark(fx, 2, v(0.0, 0.0, 0.0));
    }
    for _ in 0..30 {
        a.tick(arena.fx_world());
        b.tick(&arena.bsp);
        assert_eq!(a.last_tick_collision(), b.last_tick_collision());
        assert_eq!(a.pools[2].pos(0), b.pools[2].pos(0));
    }
    let empty = VoxelWorld::default();
    let mut c = Fx::new(&TABLE, 3);
    spark(&mut c, 1, v(0.0, 0.0, 0.0));
    c.tick(FxWorld::new(&arena.bsp, &empty));
    assert_eq!(c.last_tick_collision().solid_tests, 1);
}

#[test]
fn a_test_inside_a_volume_costs_more_and_the_total_is_capped() {
    let arena = Arena::from_level(&rock_level()).unwrap();
    let g = &arena.voxels.volumes[0];
    let inside = g.origin + v(0.5, 0.5, 0.5);
    let outside = g.origin - v(1.0, 1.0, 1.0);
    assert!(arena.fx_world().in_volume(inside) && !arena.fx_world().in_volume(outside));
    for (at, cost) in [(inside, VOXEL_TEST_COST), (outside, 1)] {
        let mut fx = Fx::new(&TABLE, 1);
        spark(&mut fx, 1, at);
        fx.tick(arena.fx_world());
        assert_eq!(fx.last_tick_collision().solid_tests, cost, "{at:?}");
    }
    // A crowd of Die sparks in the volume: the tick stops testing at the cap
    // rather than paying for every one.
    let crowd: &'static [Emitter] = Box::leak(Box::new([emitter(1, 300, Shape::Point, 0.0, 10.0)]));
    let mut fx = Fx::new(&TABLE, 1);
    fx.spawn(crowd, &args_at(inside));
    fx.tick(arena.fx_world());
    assert_eq!(fx.last_tick_collision().solid_tests, MAX_SOLID_TESTS);
}

#[test]
fn a_bounce_in_a_volume_pays_for_its_surface_query() {
    let arena = Arena::from_level(&rock_level()).unwrap();
    let mut fx = Fx::new(&TABLE, 1);
    spark(&mut fx, 2, v(5.6, 0.5, 0.3));
    fx.tick(arena.fx_world()); // 5.6 -> 6.6, past the surface at 6.1
    assert_eq!(
        fx.last_tick_collision(),
        Budget {
            solid_tests: VOXEL_TEST_COST + VOXEL_SWEEP_COST,
            sweeps: 1
        }
    );
    assert!(fx.pools[2].vel(0).x < 0.0);
}

/// A scene of the same open room, for the blast to be seen in.
fn room_scene(level: &Level) -> Scene {
    let brushes = level.static_brushes();
    let bsp = compile(&brushes).expect("a tree");
    let faces = extract_faces(&bsp, &brushes);
    let portals = Portals::build(&bsp);
    Scene::new(bsp, faces, portals, MaterialTable::default())
}

#[test]
fn a_blasts_shake_never_puts_the_eye_in_voxel_rock() {
    let level = rock_level();
    let arena = Arena::from_level(&level).unwrap();
    let scene = room_scene(&level);
    // Just off the rock, closer than the shake reaches. The surface is found
    // off the axis, where the rock test answers early (see the report).
    let (mut air, mut rock) = (4.0_f32, 7.0_f32);
    for _ in 0..30 {
        let mid = (air + rock) * 0.5;
        if arena.is_solid(v(mid, 0.5, 0.3).to_f64()) {
            rock = mid;
        } else {
            air = mid;
        }
    }
    let eye = v(air - 0.02, 0.5, 0.3);
    assert!(!arena.is_solid(eye.to_f64()));
    let (mut safe, mut bsp_only) = (CombatFx::new(), CombatFx::new());
    safe.blast(v(0.0, 0.0, 0.0), 12.0, arena.fx_world());
    bsp_only.blast(v(0.0, 0.0, 0.0), 12.0, &arena.bsp);
    let (mut shaken, mut would_enter) = (false, false);
    for _ in 0..30 {
        safe.advance(16_666_667, arena.fx_world());
        bsp_only.advance(16_666_667, &arena.bsp);
        let kick = safe.view_kick_in(&scene, eye, arena.fx_world());
        shaken |= kick.shake != Vec3::ZERO;
        assert!(!arena.is_solid((eye + kick.shake).to_f64()), "{kick:?}");
        let plain = bsp_only.view_kick_in(&scene, eye, &arena.bsp);
        would_enter |= arena.is_solid((eye + plain.shake).to_f64());
    }
    assert!(shaken, "the view shook at all");
    assert!(would_enter, "and the BSP alone would have put it in rock");
}

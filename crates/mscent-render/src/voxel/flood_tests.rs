//! The cave flood against real volumes.

use mscent_math::{Vec3, Vec3d};
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume, VoxelWorld, apply_dab};
use mscent_world::{Level, Op, compile};

use super::flood::Flood;

/// Solid rock, four chunks long on X, with a sealed cave at each end.
fn two_caves() -> VoxelWorld {
    let mut volume =
        Volume::new(Op::Subtract, 0, CellSize::Quarter, [0, 0, 0], [4, 1, 1]).expect("valid");
    let mut ctx = Context::new(&volume.clone(), &[]);
    for x in [2.0, 14.0] {
        let cave = Prim::sphere(Vec3d::new(x, 2.0, 2.0), 1.2).expect("valid");
        apply_dab(&mut volume, &mut ctx, &cave, DabOp::Carve);
    }
    build(volume)
}

fn build(volume: Volume) -> VoxelWorld {
    let level = Level {
        volumes: vec![volume],
        ..Default::default()
    };
    let bsp = compile(&level.csg_brushes()).expect("compiles");
    VoxelWorld::build(&level, &level.static_brushes(), &bsp)
}

#[test]
fn rock_between_two_caves_stops_the_flood() {
    let world = two_caves();
    let mut flood = Flood::default();
    flood.run(&world.volumes[0], Vec3::new(2.0, 2.0, 2.0));
    assert!(flood.reached(0), "the eye's own chunk");
    assert!(flood.reached(1), "the next chunk: where the rock is seen");
    assert!(!flood.reached(3), "the far cave cannot be seen from here");
}

#[test]
fn air_from_end_to_end_lets_the_flood_through() {
    let mut volume =
        Volume::new(Op::Subtract, 0, CellSize::Quarter, [0, 0, 0], [4, 1, 1]).expect("valid");
    let mut ctx = Context::new(&volume.clone(), &[]);
    let tunnel = Prim::round_cone(
        Vec3d::new(1.0, 2.0, 2.0),
        Vec3d::new(15.0, 2.0, 2.0),
        1.0,
        1.0,
    )
    .expect("valid");
    apply_dab(&mut volume, &mut ctx, &tunnel, DabOp::Carve);
    let world = build(volume);
    let mut flood = Flood::default();
    flood.run(&world.volumes[0], Vec3::new(2.0, 2.0, 2.0));
    assert!((0..4).all(|i| flood.reached(i)));
}

#[test]
fn an_eye_outside_the_box_starts_from_the_sides_it_can_see() {
    let world = two_caves();
    let mut flood = Flood::default();
    flood.run(&world.volumes[0], Vec3::new(-5.0, 2.0, 2.0));
    assert!(flood.reached(0));
    assert!(!flood.reached(3), "the far end is behind rock");
    flood.run(&world.volumes[0], Vec3::new(2.0, 2.0, 9.0));
    assert!(
        (0..4).all(|i| flood.reached(i)),
        "the whole top side is in view"
    );
}

#[test]
fn the_flood_does_not_turn_back_towards_the_eye() {
    let mut volume =
        Volume::new(Op::Subtract, 0, CellSize::Quarter, [0, 0, 0], [3, 3, 1]).expect("valid");
    let mut ctx = Context::new(&volume.clone(), &[]);
    let all =
        Prim::cuboid(Vec3d::new(-1.0, -1.0, -1.0), Vec3d::new(13.0, 13.0, 5.0)).expect("valid");
    apply_dab(&mut volume, &mut ctx, &all, DabOp::Carve);
    let world = build(volume);
    let mut flood = Flood::default();
    flood.run(&world.volumes[0], Vec3::new(6.0, 6.0, 2.0));
    assert!((0..9).all(|i| flood.reached(i)), "open air reaches it all");
}

#[test]
fn voxel_texture_axes_are_the_default_brush_mapping() {
    let axis = [
        Vec3d::new(1.0, 0.0, 0.0),
        Vec3d::new(0.0, 1.0, 0.0),
        Vec3d::new(0.0, 0.0, 1.0),
    ];
    for (i, n) in (0u8..).zip(axis) {
        let info = mscent_world::TexInfo::default_for_normal(n);
        assert_eq!(super::axis_map(i), (info.u_axis, info.v_axis));
    }
}

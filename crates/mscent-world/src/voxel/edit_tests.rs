//! Growing, moving and converting volumes, rays and crumbs.

use mscent_math::Vec3d;

use super::test_util::mesh_all;
use super::{CellSize, Context, DabOp, Prim, Volume, apply_dab, first_solid, remove_islands};
use crate::brush::{Brush, Op};

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn add_volume() -> Volume {
    Volume::new(Op::Add, 1, CellSize::Quarter, [0; 3], [1, 1, 1]).expect("valid")
}

fn with_ball(mut volume: Volume, at: Vec3d, r: f64) -> Volume {
    let shape = volume.clone();
    let ball = Prim::sphere(at, r).expect("valid");
    apply_dab(
        &mut volume,
        &mut Context::new(&shape, &[]),
        &ball,
        DabOp::Add { material: 1 },
    );
    volume
}

fn triangles(volume: &Volume) -> usize {
    mesh_all(volume, &[])
        .iter()
        .map(|(_, m)| m.triangles.len())
        .sum()
}

#[test]
fn growing_keeps_sculpting_where_it_was() {
    let ball = with_ball(add_volume(), v(2.0, 2.0, 2.0), 1.0);
    let grown = ball
        .grown_to_hold(v(-3.0, 0.0, 0.0), v(9.0, 1.0, 1.0))
        .expect("fits")
        .expect("grew");
    assert_eq!(grown.chunks(), [4, 1, 1]);
    assert_eq!(grown.bounds().0, v(-4.0, 0.0, 0.0));
    let sum = |list: Vec<_>| -> Vec<mscent_math::Vec3> {
        let mut pos: Vec<_> = list;
        pos.sort_by(|a: &mscent_math::Vec3, b| {
            (a.x, a.y, a.z)
                .partial_cmp(&(b.x, b.y, b.z))
                .expect("finite")
        });
        pos
    };
    let positions = |volume: &Volume| {
        sum(mesh_all(volume, &[])
            .iter()
            .flat_map(|(_, m)| m.vertices.iter().map(|x| x.pos))
            .collect())
    };
    assert_eq!(positions(&ball), positions(&grown));
}

#[test]
fn resizing_snaps_to_chunks_and_keeps_what_stays() {
    let ball = with_ball(add_volume(), v(2.0, 2.0, 2.0), 1.0);
    // A chunk is 4 units: 9 rounds to 8 and -1.5 to -0 (the nearest edge, 0).
    let longer = ball
        .resized_to(v(-1.5, 0.0, 0.0), v(9.0, 4.0, 4.0))
        .expect("fits");
    assert_eq!(longer.chunks(), [2, 1, 1]);
    assert_eq!(longer.bounds(), (v(0.0, 0.0, 0.0), v(8.0, 4.0, 4.0)));
    assert_eq!(triangles(&ball), triangles(&longer));
    let before = ball
        .resized_to(v(-8.0, 0.0, 0.0), v(4.0, 4.0, 4.0))
        .expect("fits");
    assert_eq!(before.bounds().0, v(-8.0, 0.0, 0.0));
    assert_eq!(triangles(&ball), triangles(&before));
    // Back to where it was is the same volume.
    let back = longer
        .resized_to(v(0.0, 0.0, 0.0), v(4.0, 4.0, 4.0))
        .expect("fits");
    assert!(back.same_box(&ball) && back.same_shape(&ball));
}

#[test]
fn shrinking_cuts_off_the_chunks_outside() {
    let ball = with_ball(add_volume(), v(2.0, 2.0, 2.0), 1.0);
    let wide = ball
        .resized_to(v(-4.0, 0.0, 0.0), v(8.0, 4.0, 4.0))
        .expect("fits");
    let right = wide
        .resized_to(v(0.0, 0.0, 0.0), v(8.0, 4.0, 4.0))
        .expect("fits");
    assert_eq!(triangles(&right), triangles(&ball));
    let left = wide
        .resized_to(v(-4.0, 0.0, 0.0), v(0.0, 4.0, 4.0))
        .expect("fits");
    assert_eq!(triangles(&left), 0);
}

#[test]
fn a_resize_that_leaves_no_chunk_or_too_many_is_refused() {
    let volume = add_volume();
    let box_of = |lo: Vec3d, hi: Vec3d| volume.resized_to(lo, hi);
    assert!(
        box_of(v(0.0, 0.0, 0.0), v(0.0, 4.0, 4.0)).is_err(),
        "no width"
    );
    assert!(
        box_of(v(4.0, 0.0, 0.0), v(0.0, 4.0, 4.0)).is_err(),
        "inverted"
    );
    assert!(
        box_of(v(0.0, 0.0, 0.0), v(4000.0, 4.0, 4.0)).is_err(),
        "too many"
    );
    let nan = f64::NAN;
    assert!(box_of(v(nan, 0.0, 0.0), v(4.0, 4.0, 4.0)).is_err());
    let far = f64::INFINITY;
    assert!(box_of(v(-far, 0.0, 0.0), v(far, 4.0, 4.0)).is_err());
}

#[test]
fn a_box_that_holds_it_already_does_not_grow() {
    let volume = add_volume();
    let inside = volume.grown_to_hold(v(0.5, 0.5, 0.5), v(1.0, 1.0, 1.0));
    assert_eq!(inside, Ok(None));
}

#[test]
fn growing_past_the_limit_is_refused() {
    let volume = add_volume();
    assert!(
        volume
            .grown_to_hold(v(0.0, 0.0, 0.0), v(4000.0, 1.0, 1.0))
            .is_err()
    );
}

#[test]
fn moving_a_volume_carries_its_sculpting() {
    let ball = with_ball(add_volume(), v(2.0, 2.0, 2.0), 1.0);
    let moved = ball.translated([8, 0, 0]).expect("fits");
    assert_eq!(moved.bounds().0, v(2.0, 0.0, 0.0));
    assert_eq!(triangles(&ball), triangles(&moved));
}

#[test]
fn converting_keeps_the_sculpted_result() {
    let brushes =
        vec![Brush::cuboid(v(-8.0, -8.0, -8.0), v(8.0, 8.0, 8.0), Op::Subtract, 0).expect("valid")];
    let mut add =
        Volume::new(Op::Add, 1, CellSize::Quarter, [-8, -8, -8], [1, 1, 1]).expect("valid");
    let shape = add.clone();
    let mut old = Context::new(&shape, &brushes);
    let ball = Prim::sphere(v(0.0, 0.0, 0.0), 1.0).expect("valid");
    apply_dab(&mut add, &mut old, &ball, DabOp::Add { material: 1 });
    let target = add.with_op(Op::Subtract);
    let mut new = Context::new(&target, &brushes);
    let sub = add.converted(Op::Subtract, &mut old, &mut new);
    assert_eq!(sub.op(), Op::Subtract);
    let mesh = |volume: &Volume| {
        let m: Vec<_> = mesh_all(volume, &brushes);
        m.iter().map(|(_, m)| m.triangles.len()).sum::<usize>()
    };
    assert_eq!(mesh(&sub), mesh(&add), "the surface inside stays");
}

#[test]
fn a_ray_stops_at_the_surface_within_half_a_cell() {
    let ball = with_ball(add_volume(), v(2.0, 2.0, 2.0), 1.0);
    let shape = ball.clone();
    let mut ctx = Context::new(&shape, &[]);
    let t = first_solid(&ball, &mut ctx, v(-5.0, 2.0, 2.0), v(1.0, 0.0, 0.0), 100.0).expect("hits");
    assert!((t - 6.0).abs() < 0.26, "at {t}");
    let away = first_solid(&ball, &mut ctx, v(-5.0, 2.0, 2.0), v(-1.0, 0.0, 0.0), 100.0);
    assert_eq!(away, None);
    let above = first_solid(&ball, &mut ctx, v(-5.0, 2.0, 3.8), v(1.0, 0.0, 0.0), 100.0);
    assert_eq!(above, None);
}

#[test]
fn only_a_crumb_is_cleared_when_a_stroke_ends() {
    let mut volume = with_ball(add_volume(), v(2.0, 2.0, 2.0), 1.0);
    volume = with_ball(volume, v(0.75, 0.75, 0.75), 0.13);
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &[]);
    let before = triangles(&volume);
    assert!(remove_islands(&mut volume, &mut ctx, &[[0, 0, 0]]));
    let after = triangles(&volume);
    assert!(after < before, "{after} of {before}");
    assert!(after > before / 2, "the ball stays: {after} of {before}");
    assert!(!remove_islands(&mut volume, &mut ctx, &[[0, 0, 0]]));
    assert_eq!(triangles(&volume), after);
}

#[test]
fn only_changed_chunks_and_those_beside_them_are_reported() {
    let plain = Volume::new(Op::Add, 1, CellSize::Quarter, [0; 3], [4, 1, 1]).expect("valid");
    let ball = with_ball(plain.clone(), v(2.0, 2.0, 2.0), 1.0);
    assert_eq!(ball.changed_chunks(&plain), [[0, 0, 0], [1, 0, 0]]);
    assert!(plain.changed_chunks(&plain).is_empty());
}

#[test]
fn shape_ignores_paint_but_not_carving() {
    let ball = with_ball(add_volume(), v(2.0, 2.0, 2.0), 1.0);
    let shape = ball.clone();
    let mut ctx = Context::new(&shape, &[]);
    let spot = Prim::sphere(v(2.0, 2.0, 2.0), 1.5).expect("valid");
    let mut painted = ball.clone();
    apply_dab(&mut painted, &mut ctx, &spot, DabOp::Paint { material: 3 });
    assert_ne!(painted, ball);
    assert!(painted.same_shape(&ball));
    let mut carved = ball.clone();
    let bite = Prim::sphere(v(3.0, 2.0, 2.0), 0.5).expect("valid");
    apply_dab(&mut carved, &mut ctx, &bite, DabOp::Carve);
    assert!(!carved.same_shape(&ball));
}

#[test]
fn an_infinite_bound_is_refused_and_does_not_overflow() {
    let volume = add_volume();
    let far = f64::INFINITY;
    assert!(
        volume
            .grown_to_hold(v(-far, 0.0, 0.0), v(far, 1.0, 1.0))
            .is_err()
    );
    assert_eq!(
        volume.grown_to_hold(v(f64::NAN, 0.0, 0.0), v(1.0, 1.0, 1.0)),
        Ok(None)
    );
}

#[test]
fn turning_over_keeps_the_look_of_rock_that_showed_its_brush() {
    let brushes = vec![
        Brush::cuboid(v(-8.0, -8.0, -8.0), v(0.0, 8.0, 8.0), Op::Subtract, 0)
            .expect("valid")
            .with_material(3),
    ];
    let sub =
        Volume::new(Op::Subtract, 1, CellSize::Quarter, [-8, -8, -8], [1, 1, 1]).expect("valid");
    let target = sub.with_op(Op::Add);
    let mut old = Context::new(&sub, &brushes);
    let mut new = Context::new(&target, &brushes);
    let add = sub.converted(Op::Add, &mut old, &mut new);
    let meshes = mesh_all(&add, &brushes);
    let materials: Vec<u8> = (meshes.iter())
        .flat_map(|(_, m)| m.materials.iter().copied())
        .collect();
    assert!(!materials.is_empty());
    assert!(materials.iter().all(|&m| m == 3), "{materials:?}");
}

#[test]
fn the_chunks_near_a_shape_are_those_that_read_it() {
    let volume = Volume::new(Op::Add, 1, CellSize::Quarter, [0; 3], [4, 1, 1]).expect("valid");
    assert_eq!(
        volume.chunks_near(v(3.9, 1.0, 1.0), v(4.1, 2.0, 2.0)),
        [[0, 0, 0], [1, 0, 0]]
    );
    assert!(
        volume
            .chunks_near(v(40.0, 1.0, 1.0), v(41.0, 2.0, 2.0))
            .is_empty()
    );
}

//! Flatten, Roughen, Copy and Stamp, and the shapes generators add.

use mscent_math::Vec3d;

use super::test_util::{gyroid, mesh_all};
use super::{CellSize, Context, DabOp, Prim, Volume, apply_dab, capture, fill, stamp};
use crate::brush::Op;

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn volume() -> Volume {
    Volume::new(Op::Add, 0, CellSize::Quarter, [0; 3], [1, 1, 1]).expect("valid")
}

fn sphere(c: Vec3d, r: f64) -> Prim {
    Prim::sphere(c, r).expect("valid")
}

const ADD: DabOp = DabOp::Add { material: 1 };

fn points(vol: &Volume) -> Vec<Vec3d> {
    mesh_all(vol, &[])[0]
        .1
        .vertices
        .iter()
        .map(|q| q.pos.to_f64())
        .collect()
}

#[test]
fn flatten_cuts_a_hump_and_fills_a_pit_down_to_the_plane() {
    let mut vol = volume();
    let mut ctx = Context::new(&vol, &[]);
    let block = Prim::cuboid(v(0.5, 0.5, 0.5), v(3.5, 3.5, 3.0)).expect("valid");
    apply_dab(&mut vol, &mut ctx, &block, ADD);
    apply_dab(&mut vol, &mut ctx, &sphere(v(2.0, 2.0, 3.0), 0.8), ADD);
    apply_dab(
        &mut vol,
        &mut ctx,
        &sphere(v(3.0, 3.0, 3.0), 0.5),
        DabOp::Carve,
    );
    let tallest = |vol: &Volume| points(vol).iter().map(|p| p.z).fold(f64::MIN, f64::max);
    // The lowest the surface goes in the column the pit was dug in.
    let pit_floor = |vol: &Volume| {
        points(vol)
            .iter()
            .filter(|p| (p.x - 3.0).abs() < 0.3 && (p.y - 3.0).abs() < 0.3 && p.z > 2.0)
            .map(|p| p.z)
            .fold(f64::MAX, f64::min)
    };
    assert!(pit_floor(&vol) < 2.7, "the pit is there");
    assert!(tallest(&vol) > 3.6, "the hump is there");
    let flatten = DabOp::Flatten {
        material: 2,
        origin: v(0.0, 0.0, 3.0),
        normal: v(0.0, 0.0, 1.0),
    };
    for _ in 0..3 {
        for at in [v(2.0, 2.0, 3.0), v(3.0, 3.0, 3.0)] {
            apply_dab(&mut vol, &mut ctx, &sphere(at, 1.4), flatten);
        }
    }
    assert!(tallest(&vol) < 3.2, "the hump is gone: {}", tallest(&vol));
    assert!(
        pit_floor(&vol) > 2.9,
        "the pit is filled: {}",
        pit_floor(&vol)
    );
}

#[test]
fn roughen_is_seeded_and_moves_the_surface() {
    let rough = |seed: u32| {
        let mut vol = volume();
        let mut ctx = Context::new(&vol, &[]);
        apply_dab(&mut vol, &mut ctx, &sphere(v(2.0, 2.0, 2.0), 1.5), ADD);
        let smooth = vol.clone();
        let op = DabOp::Roughen {
            seed,
            amplitude: 24,
            material: 1,
        };
        apply_dab(&mut vol, &mut ctx, &sphere(v(2.0, 2.0, 2.0), 1.9), op);
        (smooth, vol)
    };
    let (smooth, a) = rough(5);
    assert_ne!(a, smooth, "the surface moved");
    assert_eq!(a, rough(5).1, "the same seed bumps the same places");
    assert_ne!(a, rough(6).1, "another seed bumps others");
}

#[test]
fn a_stamp_reproduces_what_was_copied_and_leaves_the_rest() {
    let mut source = volume();
    let mut ctx = Context::new(&source, &[]);
    fill(&mut source, &mut ctx, gyroid, 3);
    let clip = capture(&source, &mut ctx, v(2.0, 2.0, 2.0), 1.25).expect("captures");
    assert_eq!(clip.cell(), CellSize::Quarter);
    assert!(clip.reach() >= 1.25);

    let mut target = volume();
    let mut target_ctx = Context::new(&target, &[]);
    let chunks = stamp(&mut target, &mut target_ctx, &clip, v(2.0, 2.0, 2.0));
    assert_eq!(chunks, vec![[0, 0, 0]]);
    let back = capture(&target, &mut target_ctx, v(2.0, 2.0, 2.0), 1.25).expect("captures");
    assert_eq!(back, clip, "pasted samples are the copied samples");
    // The corner of the clip's box lies outside the ball, so nothing was written.
    let corner = capture(&target, &mut target_ctx, v(3.2, 3.2, 3.2), 0.25).expect("captures");
    let air = capture(&volume(), &mut target_ctx, v(3.2, 3.2, 3.2), 0.25).expect("captures");
    assert_eq!(corner, air);
}

#[test]
fn a_stamp_in_the_wrong_place_or_for_the_wrong_volume_does_nothing() {
    let mut source = volume();
    let mut ctx = Context::new(&source, &[]);
    fill(&mut source, &mut ctx, gyroid, 3);
    let clip = capture(&source, &mut ctx, v(2.0, 2.0, 2.0), 1.0).expect("captures");
    let mut target = volume();
    let mut target_ctx = Context::new(&target, &[]);
    assert!(stamp(&mut target, &mut target_ctx, &clip, v(900.0, 0.0, 0.0)).is_empty());
    assert!(stamp(&mut target, &mut target_ctx, &clip, v(f64::NAN, 0.0, 0.0)).is_empty());
    let half = Volume::new(Op::Add, 0, CellSize::Half, [0; 3], [1, 1, 1]).expect("valid");
    let mut half_ctx = Context::new(&half, &[]);
    let mut half = half;
    assert!(stamp(&mut half, &mut half_ctx, &clip, v(2.0, 2.0, 2.0)).is_empty());
    assert!(capture(&source, &mut half_ctx, v(2.0, 2.0, 2.0), 1.0).is_none());
    assert!(capture(&source, &mut ctx, v(2.0, 2.0, 2.0), f64::NAN).is_none());
    assert!(capture(&source, &mut ctx, v(f64::INFINITY, 0.0, 0.0), 1.0).is_none());
}

#[test]
fn spheroids_and_hex_prisms_have_the_right_sign_everywhere_that_matters() {
    let c = v(1.0, 2.0, 3.0);
    let up = v(0.0, 0.0, 2.0);
    let ball = Prim::spheroid(c, up, 2.0, 1.0).expect("valid");
    assert!(ball.distance(c) < 0.0);
    assert!(ball.distance(c + v(0.0, 0.0, 1.9)) < 0.0);
    assert!(ball.distance(c + v(0.0, 0.0, 2.1)) > 0.0);
    assert!(ball.distance(c + v(0.9, 0.0, 0.0)) < 0.0);
    assert!(ball.distance(c + v(1.1, 0.0, 0.0)) > 0.0);
    let (lo, hi) = ball.bounds();
    assert!(lo.z <= 1.0 && hi.z >= 5.0 && lo.x <= 0.0 && hi.x >= 2.0);

    let cell = Prim::hex_prism(c, v(0.0, 0.0, 1.0), v(1.0, 0.0, 0.0), 1.0, 0.5).expect("valid");
    assert!(cell.distance(c) < 0.0);
    // Flats at 1 along y, corners at 1.1547 along x, and 0.5 either way along z.
    assert!(cell.distance(c + v(0.0, 0.95, 0.0)) < 0.0);
    assert!(cell.distance(c + v(0.0, 1.05, 0.0)) > 0.0);
    assert!(cell.distance(c + v(1.12, 0.0, 0.0)) < 0.0);
    assert!(cell.distance(c + v(1.2, 0.0, 0.0)) > 0.0);
    assert!(cell.distance(c + v(0.0, 0.0, 0.45)) < 0.0);
    assert!(cell.distance(c + v(0.0, 0.0, 0.55)) > 0.0);
    let (lo, hi) = cell.bounds();
    assert!(lo.x <= c.x - 1.1547 && hi.x >= c.x + 1.1547 && hi.z >= c.z + 0.5);
}

#[test]
fn hostile_spheroids_and_prisms_are_refused() {
    let ok = v(0.0, 0.0, 0.0);
    let z = v(0.0, 0.0, 1.0);
    assert!(Prim::spheroid(ok, ok, 1.0, 1.0).is_none(), "no axis");
    assert!(Prim::spheroid(ok, v(f64::NAN, 0.0, 1.0), 1.0, 1.0).is_none());
    assert!(Prim::spheroid(ok, z, 0.0, 1.0).is_none());
    assert!(Prim::spheroid(ok, z, 1.0, f64::INFINITY).is_none());
    assert!(Prim::spheroid(v(1.0e300, 0.0, 0.0), z, 1.0, 1.0).is_none());
    assert!(
        Prim::hex_prism(ok, z, z, 1.0, 1.0).is_none(),
        "corner along the axis"
    );
    assert!(Prim::hex_prism(ok, z, ok, 1.0, 1.0).is_none());
    assert!(Prim::hex_prism(ok, ok, v(1.0, 0.0, 0.0), 1.0, 1.0).is_none());
    assert!(Prim::hex_prism(ok, z, v(1.0, 0.0, 0.0), -1.0, 1.0).is_none());
    assert!(Prim::hex_prism(ok, z, v(1.0, 0.0, 0.0), 1.0, f64::NAN).is_none());
}

use std::sync::Arc;

use mscent_math::Vec3d;

use super::test_util::{coords, gyroid, mesh_all};
use super::{CHUNK_EDGE, CellSize, Chunk, Context, DabOp, Prim, Volume, apply_dab, fill};
use crate::brush::{Brush, Op};

fn volume(chunks: [u16; 3]) -> Volume {
    Volume::new(Op::Add, 0, CellSize::Quarter, [0; 3], chunks).expect("valid")
}

fn sphere(x: f64, y: f64, z: f64, r: f64) -> Prim {
    Prim::sphere(Vec3d::new(x, y, z), r).expect("valid")
}

fn detail_chunks(v: &Volume) -> usize {
    coords(v)
        .iter()
        .filter(|c| matches!(v.chunk(**c), Some(Chunk::Detail(_))))
        .count()
}

const ADD: DabOp = DabOp::Add { material: 1 };

#[test]
fn a_dab_copies_only_chunks_it_changes() {
    let mut v = volume([4, 1, 1]);
    let mut ctx = Context::new(&v, &[]);
    fill(&mut v, &mut ctx, gyroid, 0);
    let before = v.clone();
    apply_dab(&mut v, &mut ctx, &sphere(2.0, 2.0, 2.0, 0.75), DabOp::Carve);
    assert_ne!(v, before);
    let shared = coords(&v)
        .iter()
        .filter(|c| match (v.chunk(**c), before.chunk(**c)) {
            (Some(Chunk::Detail(a)), Some(Chunk::Detail(b))) => Arc::ptr_eq(a, b),
            _ => false,
        })
        .count();
    assert_eq!(shared, 3, "three of four chunks are untouched");
}

#[test]
fn a_clone_is_an_exact_undo_step() {
    let mut v = volume([2, 2, 2]);
    let mut ctx = Context::new(&v, &[]);
    apply_dab(&mut v, &mut ctx, &sphere(4.0, 4.0, 4.0, 2.0), ADD);
    let held = v.clone();
    let meshes = mesh_all(&v, &[]);
    apply_dab(&mut v, &mut ctx, &sphere(3.0, 4.0, 4.0, 1.5), DabOp::Carve);
    assert_ne!(mesh_all(&v, &[]), meshes);
    assert_eq!(mesh_all(&held, &[]), meshes, "the old state is untouched");
}

#[test]
fn a_dab_that_changes_nothing_reports_nothing() {
    let mut v = volume([1, 1, 1]);
    let mut ctx = Context::new(&v, &[]);
    assert!(apply_dab(&mut v, &mut ctx, &sphere(2.0, 2.0, 2.0, 1.0), DabOp::Carve).is_empty());
    assert_eq!(detail_chunks(&v), 0, "carving air allocates nothing");
    let far = sphere(100.0, 100.0, 100.0, 1.0);
    assert!(apply_dab(&mut v, &mut ctx, &far, ADD).is_empty());
}

#[test]
fn removing_everything_returns_chunks_to_the_uniform_form() {
    let mut v = volume([1, 1, 1]);
    let mut ctx = Context::new(&v, &[]);
    let original = v.clone();
    apply_dab(
        &mut v,
        &mut ctx,
        &sphere(2.0, 2.0, 2.0, 1.0),
        DabOp::Add { material: 0 },
    );
    assert_eq!(detail_chunks(&v), 1);
    let all = Prim::cuboid(Vec3d::new(-9.0, -9.0, -9.0), Vec3d::new(9.0, 9.0, 9.0)).expect("ok");
    apply_dab(&mut v, &mut ctx, &all, DabOp::Carve);
    assert_eq!(detail_chunks(&v), 0);
    assert_eq!(mesh_all(&v, &[]), mesh_all(&original, &[]));
}

#[test]
fn dabs_cannot_reach_the_ring_so_the_surface_stays_in_the_box() {
    let mut v = volume([1, 1, 1]);
    let mut ctx = Context::new(&v, &[]);
    apply_dab(&mut v, &mut ctx, &sphere(0.0, 0.0, 0.0, 50.0), ADD);
    let (lo, hi) = v.bounds();
    let cell = v.cell().units() as f32;
    let meshes = mesh_all(&v, &[]);
    assert!(!meshes[0].1.triangles.is_empty());
    for vert in &meshes[0].1.vertices {
        let p = vert.pos;
        for (c, (l, h)) in [
            (p.x, (lo.x, hi.x)),
            (p.y, (lo.y, hi.y)),
            (p.z, (lo.z, hi.z)),
        ] {
            assert!(c >= l as f32 - cell && c <= h as f32 + cell, "{p:?}");
        }
    }
}

#[test]
fn paint_changes_material_and_never_shape() {
    let mut v = volume([1, 1, 1]);
    let mut ctx = Context::new(&v, &[]);
    apply_dab(&mut v, &mut ctx, &sphere(2.0, 2.0, 2.0, 1.5), ADD);
    let before = mesh_all(&v, &[]);
    let dab = sphere(2.0, 2.0, 3.5, 1.0);
    apply_dab(&mut v, &mut ctx, &dab, DabOp::Paint { material: 7 });
    let after = mesh_all(&v, &[]);
    assert_eq!(before[0].1.vertices, after[0].1.vertices);
    assert_eq!(before[0].1.triangles, after[0].1.triangles);
    assert!(after[0].1.materials.contains(&7));
    assert!(after[0].1.materials.contains(&1));
}

#[test]
fn smoothing_rounds_a_sharp_corner_without_moving_distant_surface() {
    let mut v = volume([1, 1, 1]);
    let mut ctx = Context::new(&v, &[]);
    let block = Prim::cuboid(Vec3d::new(1.0, 1.0, 1.0), Vec3d::new(3.0, 3.0, 3.0)).expect("ok");
    apply_dab(&mut v, &mut ctx, &block, ADD);
    let corner = Vec3d::new(3.0, 3.0, 3.0);
    let positions = |v: &Volume| -> Vec<Vec3d> {
        mesh_all(v, &[])[0]
            .1
            .vertices
            .iter()
            .map(|q| q.pos.to_f64())
            .collect()
    };
    // How far the surface sticks out toward the smoothed corner.
    let reach = |pts: &[Vec3d]| {
        pts.iter()
            .map(|p| (p.x + p.y + p.z) / 3.0_f64.sqrt())
            .fold(f64::MIN, f64::max)
    };
    let sharp = positions(&v);
    let dab = Prim::sphere(corner, 0.9).expect("valid");
    for _ in 0..6 {
        apply_dab(&mut v, &mut ctx, &dab, DabOp::Smooth);
    }
    let smooth = positions(&v);
    assert!(reach(&smooth) < reach(&sharp) - 0.03, "{}", reach(&smooth));
    for p in sharp.iter().filter(|&&p| (p - corner).length() > 2.0) {
        assert!(smooth.contains(p), "{p:?} moved");
    }
}

#[test]
fn hostile_shapes_are_refused_or_clipped() {
    let ok = Vec3d::new(0.0, 0.0, 0.0);
    assert!(Prim::sphere(Vec3d::new(f64::NAN, 0.0, 0.0), 1.0).is_none());
    assert!(Prim::sphere(ok, 0.0).is_none());
    assert!(Prim::sphere(ok, f64::INFINITY).is_none());
    assert!(Prim::round_cone(ok, ok, -1.0, 1.0).is_none());
    assert!(Prim::cuboid(ok, ok).is_none());
    let mut v = volume([1, 1, 1]);
    let mut ctx = Context::new(&v, &[]);
    let huge = Prim::sphere(ok, 1.0e300).expect("finite");
    assert_eq!(apply_dab(&mut v, &mut ctx, &huge, ADD), vec![[0, 0, 0]]);
    let far = Prim::sphere(Vec3d::new(1.0e300, -1.0e300, 0.0), 1.0e299).expect("finite");
    assert!(apply_dab(&mut v, &mut ctx, &far, DabOp::Carve).is_empty());
}

#[test]
fn a_context_for_another_volume_changes_nothing() {
    let mut v = volume([1, 1, 1]);
    let other = volume([2, 1, 1]);
    let mut ctx = Context::new(&other, &[]);
    assert!(apply_dab(&mut v, &mut ctx, &sphere(2.0, 2.0, 2.0, 1.0), ADD).is_empty());
    assert!(fill(&mut v, &mut ctx, gyroid, 0).is_empty());
    assert_eq!(detail_chunks(&v), 0);
}

#[test]
fn a_dab_at_a_chunk_edge_also_dirties_the_neighbour_that_pads_it() {
    let mut v = volume([3, 1, 1]);
    let mut ctx = Context::new(&v, &[]);
    // Sample 15 of chunk 0, which chunk 1 holds as padding.
    let x = (CHUNK_EDGE as f64 - 1.0) * v.cell().units();
    let chunks = apply_dab(&mut v, &mut ctx, &sphere(x, 2.0, 2.0, 0.05), ADD);
    assert!(
        chunks.contains(&[0, 0, 0]) && chunks.contains(&[1, 0, 0]),
        "{chunks:?}"
    );
    assert!(!chunks.contains(&[2, 0, 0]), "{chunks:?}");
}

#[test]
fn a_carve_of_brush_rock_is_stored_over_the_base() {
    let wall = vec![
        Brush::cuboid(
            Vec3d::new(-9.0, -9.0, 2.0),
            Vec3d::new(9.0, 9.0, 9.0),
            Op::Subtract,
            0,
        )
        .expect("valid"),
    ];
    let mut v = Volume::new(Op::Subtract, 1, CellSize::Quarter, [0; 3], [1, 1, 1]).expect("valid");
    assert_eq!(detail_chunks(&v), 0, "the base is not stored");
    let mut ctx = Context::new(&v, &wall);
    apply_dab(&mut v, &mut ctx, &sphere(2.0, 2.0, 1.5, 1.0), DabOp::Carve);
    assert_eq!(detail_chunks(&v), 1);
    let meshes = mesh_all(&v, &wall);
    let deepest = meshes[0]
        .1
        .vertices
        .iter()
        .map(|q| q.pos.z)
        .filter(|&z| z < 1.9)
        .fold(f32::MAX, f32::min);
    assert!((deepest - 0.5).abs() < 0.13, "pit floor at {deepest}");
}

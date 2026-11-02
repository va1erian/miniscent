use mscent_math::Vec3d;
use mscent_math::rng::Rng;

use super::test_util::{
    Meshes, assert_shared_vertices_agree, coords, global_cell, gyroid, mesh_all, open_edges,
    random_point,
};
use super::{AirMask, CellSize, Context, DabOp, Face, Lod, Mesher, Prim, Volume, apply_dab, fill};
use crate::brush::Op;

fn add_volume(chunks: [u16; 3]) -> Volume {
    Volume::new(Op::Add, 0, CellSize::Quarter, [0; 3], chunks).expect("valid")
}

fn gyroid_volume() -> Volume {
    let mut v = add_volume([3, 3, 2]);
    let mut ctx = Context::new(&v, &[]);
    fill(&mut v, &mut ctx, gyroid, 1);
    v
}

fn triangle_count(meshes: &Meshes) -> usize {
    meshes.iter().map(|(_, m)| m.triangles.len()).sum()
}

#[test]
fn an_untouched_add_volume_has_no_surface() {
    let v = add_volume([2, 2, 2]);
    for (_, m) in mesh_all(&v, &[]) {
        assert!(m.triangles.is_empty() && m.vertices.is_empty());
        assert_eq!(m.air, AirMask::ALL);
    }
}

#[test]
fn a_sphere_across_chunks_is_closed_and_the_right_size() {
    let mut v = add_volume([2, 2, 2]);
    let mut ctx = Context::new(&v, &[]);
    let centre = Vec3d::new(4.0, 4.0, 4.0);
    let sphere = Prim::sphere(centre, 2.5).expect("valid");
    apply_dab(&mut v, &mut ctx, &sphere, DabOp::Add { material: 2 });
    let meshes = mesh_all(&v, &[]);
    assert!(triangle_count(&meshes) > 500, "{}", triangle_count(&meshes));
    assert_eq!(open_edges(&meshes), Vec::new());
    assert_shared_vertices_agree(&meshes);
    for (_, m) in &meshes {
        assert!(m.materials.iter().all(|&id| id == 2));
        for vert in &m.vertices {
            let out = vert.pos.to_f64() - centre;
            let r = out.length();
            assert!((r - 2.5).abs() < 0.05, "radius {r}");
            let n = out.normalized().expect("off centre");
            assert!(vert.normal.to_f64().dot(n) > 0.9, "normal points outward");
        }
    }
}

#[test]
fn triangles_wind_counter_clockwise_from_the_air_side() {
    let mut v = add_volume([1, 1, 1]);
    let mut ctx = Context::new(&v, &[]);
    let centre = Vec3d::new(2.0, 2.0, 2.0);
    let sphere = Prim::sphere(centre, 1.2).expect("valid");
    apply_dab(&mut v, &mut ctx, &sphere, DabOp::Add { material: 0 });
    let meshes = mesh_all(&v, &[]);
    let m = &meshes[0].1;
    assert!(!m.triangles.is_empty());
    for t in &m.triangles {
        let [a, b, c] = t.map(|i| m.vertices[usize::from(i)].pos);
        let facing = (b - a).cross(c - a).to_f64().dot(a.to_f64() - centre);
        assert!(facing > 0.0, "triangle {t:?} faces inward");
    }
}

#[test]
fn a_field_touching_every_side_still_closes_against_the_ring() {
    let meshes = mesh_all(&gyroid_volume(), &[]);
    assert!(triangle_count(&meshes) > 2000);
    assert_eq!(open_edges(&meshes), Vec::new());
    assert_shared_vertices_agree(&meshes);
}

#[test]
fn a_cave_closes_against_rock_on_every_side_of_its_box() {
    let mut v = Volume::new(Op::Subtract, 0, CellSize::Quarter, [0; 3], [3, 3, 2]).expect("valid");
    let mut ctx = Context::new(&v, &[]);
    fill(&mut v, &mut ctx, gyroid, 1);
    let meshes = mesh_all(&v, &[]);
    assert!(triangle_count(&meshes) > 2000);
    assert_eq!(open_edges(&meshes), Vec::new());
    assert_shared_vertices_agree(&meshes);
}

#[test]
fn random_dabs_never_open_the_surface() {
    let mut rng = Rng::new(11);
    let mut v = add_volume([3, 2, 2]);
    let mut ctx = Context::new(&v, &[]);
    for i in 0..60 {
        let p = random_point(&mut rng, &v);
        let r = rng.range_f64(0.2, 2.5);
        let prim = Prim::sphere(p, r).expect("valid");
        let op = match i % 4 {
            0 | 1 => DabOp::Add { material: 1 },
            2 => DabOp::Carve,
            _ => DabOp::Smooth,
        };
        apply_dab(&mut v, &mut ctx, &prim, op);
    }
    let meshes = mesh_all(&v, &[]);
    assert!(triangle_count(&meshes) > 100);
    assert_eq!(open_edges(&meshes), Vec::new());
    assert_shared_vertices_agree(&meshes);
}

#[test]
fn any_mix_of_detail_levels_stays_closed_and_is_smaller() {
    let full = mesh_all(&gyroid_volume(), &[]);
    let level = |lod| -> Meshes { full.iter().map(|(c, m)| (*c, m.clustered(lod))).collect() };
    let (half, quarter) = (level(Lod::Half), level(Lod::Quarter));
    assert!(triangle_count(&half) * 10 < triangle_count(&full) * 7);
    assert!(triangle_count(&quarter) < triangle_count(&half));
    assert_eq!(open_edges(&half), Vec::new());
    assert_eq!(open_edges(&quarter), Vec::new());
    let mut rng = Rng::new(5);
    for _ in 0..8 {
        let mixed: Meshes = full
            .iter()
            .map(|(c, m)| {
                let chosen = match rng.below(3) {
                    0 => m.clone(),
                    1 => m.clustered(Lod::Half),
                    _ => m.clustered(Lod::Quarter),
                };
                (*c, chosen)
            })
            .collect();
        assert_eq!(open_edges(&mixed), Vec::new());
    }
}

#[test]
fn border_vertices_do_not_move_in_any_detail_level() {
    for (_, m) in mesh_all(&gyroid_volume(), &[]) {
        let border = |c: [i8; 3]| c.iter().any(|&v| !(0..15).contains(&v));
        for lod in [Lod::Half, Lod::Quarter] {
            let c = m.clustered(lod);
            for v in m.vertices.iter().filter(|v| border(v.cell)) {
                let kept = c.vertices.iter().find(|q| q.cell == v.cell).expect("kept");
                assert_eq!(kept, v);
            }
        }
    }
}

#[test]
fn detail_levels_keep_triangles_and_materials_parallel_and_indices_in_range() {
    for (_, m) in mesh_all(&gyroid_volume(), &[]) {
        for lod in [Lod::Half, Lod::Quarter] {
            let c = m.clustered(lod);
            assert_eq!(c.triangles.len(), c.materials.len());
            assert_eq!(c.air, m.air);
            for t in &c.triangles {
                assert!(t.iter().all(|&i| usize::from(i) < c.vertices.len()));
            }
        }
    }
}

#[test]
fn meshing_the_same_volume_twice_gives_the_same_bits() {
    let v = gyroid_volume();
    assert_eq!(mesh_all(&v, &[]), mesh_all(&v.clone(), &[]));
}

#[test]
fn a_chunk_outside_the_volume_or_a_foreign_context_gives_no_mesh() {
    let v = add_volume([1, 1, 1]);
    let other = add_volume([2, 1, 1]);
    let mut mesher = Mesher::new();
    let mut own = Context::new(&v, &[]);
    let mut foreign = Context::new(&other, &[]);
    assert!(mesher.mesh(&v, &mut own, [1, 0, 0]).is_none());
    assert!(mesher.mesh(&v, &mut foreign, [0, 0, 0]).is_none());
}

#[test]
fn air_mask_says_which_faces_connect() {
    // A solid slab across the middle of Z keeps above and below apart.
    let mut v = add_volume([1, 1, 1]);
    let mut ctx = Context::new(&v, &[]);
    let slab = Prim::cuboid(Vec3d::new(-1.0, -1.0, 1.5), Vec3d::new(5.0, 5.0, 2.5)).expect("ok");
    apply_dab(&mut v, &mut ctx, &slab, DabOp::Add { material: 0 });
    let mut mesher = Mesher::new();
    let m = mesher.mesh(&v, &mut ctx, [0, 0, 0]).expect("exists");
    assert!(!m.air.joins(Face::MinZ, Face::MaxZ));
    assert!(m.air.joins(Face::MinX, Face::MaxX));
    assert!(m.air.joins(Face::MinZ, Face::MinX));
    assert!(!m.air.joins(Face::MinX, Face::MinX), "a face and itself");
    // A shaft through the slab joins them.
    let (a, b) = (Vec3d::new(2.0, 2.0, 1.0), Vec3d::new(2.0, 2.0, 3.0));
    let shaft = Prim::round_cone(a, b, 0.5, 0.5).expect("valid");
    apply_dab(&mut v, &mut ctx, &shaft, DabOp::Carve);
    let m = mesher.mesh(&v, &mut ctx, [0, 0, 0]).expect("exists");
    assert!(m.air.joins(Face::MinZ, Face::MaxZ));
}

#[test]
fn only_reported_chunks_change_when_a_dab_lands() {
    let mut rng = Rng::new(3);
    let mut v = gyroid_volume();
    let mut ctx = Context::new(&v, &[]);
    let mut mesher = Mesher::new();
    let mut held = mesh_all(&v, &[]);
    for i in 0..40 {
        let p = random_point(&mut rng, &v);
        let prim = Prim::sphere(p, rng.range_f64(0.2, 1.5)).expect("valid");
        let op = if i % 2 == 0 {
            DabOp::Carve
        } else {
            DabOp::Add { material: 3 }
        };
        for c in apply_dab(&mut v, &mut ctx, &prim, op) {
            let slot = coords(&v).iter().position(|x| *x == c).expect("in range");
            held[slot].1 = mesher.mesh(&v, &mut ctx, c).expect("exists");
        }
        assert_eq!(held, mesh_all(&v, &[]), "after dab {i}");
    }
}

#[test]
fn a_border_vertex_is_named_the_same_from_both_sides() {
    assert_eq!(
        global_cell([0, 0, 0], [15, 3, 4]),
        global_cell([1, 0, 0], [-1, 3, 4])
    );
}

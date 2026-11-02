use mscent_math::rng::Rng;
use mscent_math::{DPlane, Vec3d, epsilon};

use super::block::block_index;
use super::test_util::{assert_shared_vertices_agree, coords, mesh_all};
use super::{CHUNK_EDGE, CellSize, Context, DabOp, Mesher, Prim, Volume, VolumeError, apply_dab};
use crate::brush::{Brush, Op};
use crate::bsp::Bsp;
use crate::csg::compile;

fn cuboid(lo: [f64; 3], hi: [f64; 3], op: Op, order: u32) -> Brush {
    let v = |a: [f64; 3]| Vec3d::new(a[0], a[1], a[2]);
    Brush::cuboid(v(lo), v(hi), op, order).expect("valid box")
}

/// A sloped solid, so not every plane runs along the lattice.
fn ramp(op: Op, order: u32) -> Brush {
    let plane = |n: [f64; 3], d: f64| DPlane::new(Vec3d::new(n[0], n[1], n[2]), d);
    Brush::new(
        &[
            plane([0.6, 0.0, 0.8], 7.0),
            plane([-1.0, 0.0, 0.0], 0.0),
            plane([0.0, 1.0, 0.0], 8.0),
            plane([0.0, -1.0, 0.0], 0.0),
            plane([0.0, 0.0, -1.0], -1.0),
        ],
        op,
        order,
    )
    .expect("valid ramp")
}

fn near_any_plane(brushes: &[Brush], extra: Option<&Brush>, p: Vec3d, within: f64) -> bool {
    brushes.iter().chain(extra).any(|b| {
        b.planes()
            .iter()
            .any(|pl| pl.signed_distance(p).abs() <= within)
    })
}

/// Walks every sample of `volume`, its own and the ring, as `(global, world,
/// resolved)`. Shared samples come by once per chunk that holds them.
fn each_sample(volume: &Volume, brushes: &[Brush], mut visit: impl FnMut([i32; 3], Vec3d, i8)) {
    let mut ctx = Context::new(volume, brushes);
    let mut mesher = Mesher::new();
    let edge = CHUNK_EDGE as i32;
    let (lo, _) = volume.bounds();
    let cell = volume.cell().units();
    for c in coords(volume) {
        mesher.mesh(volume, &mut ctx, c).expect("exists");
        let first = c.map(|v| i32::from(v) * edge);
        for z in -1..=edge {
            for y in -1..=edge {
                for x in -1..=edge {
                    let g = [first[0] + x, first[1] + y, first[2] + z];
                    let p = Vec3d::new(
                        lo.x + f64::from(g[0]) * cell,
                        lo.y + f64::from(g[1]) * cell,
                        lo.z + f64::from(g[2]) * cell,
                    );
                    visit(g, p, mesher.block().density[block_index(x, y, z)]);
                }
            }
        }
    }
}

/// A level with a room, a pillar and a ramp, then two brushes after the volume.
fn level() -> Vec<Brush> {
    vec![
        cuboid([-8.0, -8.0, 0.0], [24.0, 24.0, 8.0], Op::Subtract, 0),
        cuboid([3.0, 3.0, 0.0], [5.0, 5.0, 8.0], Op::Add, 2),
        ramp(Op::Add, 4),
        cuboid([1.0, 6.0, 2.0], [3.0, 12.0, 4.0], Op::Subtract, 10),
        cuboid([10.0, 2.0, 0.0], [11.5, 3.0, 2.0], Op::Add, 12),
    ]
}

/// Cells of 0.5 from (0, 0, -2), reaching below the room's floor and over the
/// pillar, the ramp and both later brushes.
fn subtract_volume() -> Volume {
    Volume::new(Op::Subtract, 6, CellSize::Half, [0, 0, -4], [2, 2, 2]).expect("valid")
}

fn with_shell(brushes: &[Brush], volume: &Volume) -> Bsp {
    let mut all = brushes.to_vec();
    all.push(volume.shell().expect("subtract").expect("valid"));
    compile(&all).expect("compiles")
}

#[test]
fn an_untouched_subtract_volume_leaves_the_world_as_solid_as_it_was() {
    let brushes = level();
    let volume = subtract_volume();
    let plain = compile(&brushes).expect("compiles");
    let carved = with_shell(&brushes, &volume);
    let shell = volume.shell().expect("subtract").expect("valid");
    let samples = volume.chunks().map(|c| i32::from(c) * CHUNK_EDGE as i32);
    let (mut solid, mut air) = (0, 0);
    each_sample(&volume, &brushes, |g, p, value| {
        let ring = (0..3).any(|a| g[a] < 0 || g[a] >= samples[a]);
        if ring || near_any_plane(&brushes, Some(&shell), p, epsilon::PLANE_EPSILON_F64) {
            return;
        }
        let hybrid = carved.is_solid(p) || value >= 0;
        assert_eq!(hybrid, plain.is_solid(p), "at {p:?}, sample {value}");
        if plain.is_solid(p) {
            solid += 1;
        } else {
            air += 1;
        }
    });
    assert!(solid > 1000 && air > 1000, "both kinds seen: {solid} {air}");
}

#[test]
fn the_ring_shows_the_world_outside_the_box() {
    let brushes = level();
    let volume = subtract_volume();
    let plain = compile(&brushes).expect("compiles");
    let samples = volume.chunks().map(|c| i32::from(c) * CHUNK_EDGE as i32);
    let band = epsilon::VOXEL_SEAM_BIAS_CELLS * volume.cell().units() * 2.0;
    let mut seen = 0;
    each_sample(&volume, &brushes, |g, p, value| {
        let ring = (0..3).any(|a| g[a] < 0 || g[a] >= samples[a]);
        if ring && !near_any_plane(&brushes, None, p, band) {
            assert_eq!(value >= 0, plain.is_solid(p), "ring at {p:?}");
            seen += 1;
        }
    });
    assert!(seen > 500, "{seen}");
}

#[test]
fn a_wall_through_a_volume_stays_flat_and_slides_behind_at_the_ring() {
    // The floor of the room at z = 0 runs right through the box.
    let brushes = vec![cuboid(
        [-16.0, -16.0, 0.0],
        [32.0, 32.0, 8.0],
        Op::Subtract,
        0,
    )];
    let volume =
        Volume::new(Op::Subtract, 1, CellSize::Quarter, [0, 0, -8], [1, 1, 1]).expect("valid");
    let meshes = mesh_all(&volume, &brushes);
    let mesh = &meshes[0].1;
    assert!(mesh.triangles.len() > 400, "{}", mesh.triangles.len());
    let bias = (epsilon::VOXEL_SEAM_BIAS_CELLS * volume.cell().units()) as f32;
    for v in &mesh.vertices {
        if v.cell.iter().any(|&c| !(0..15).contains(&c)) {
            assert!(v.pos.z <= 0.0 && v.pos.z >= -bias * 2.0, "{:?}", v.pos);
        } else {
            assert_eq!(v.pos.z, 0.0, "flat wall at {:?}", v.pos);
            assert_eq!(v.normal.z, 1.0, "normal at {:?}", v.pos);
        }
    }
}

#[test]
fn a_wall_runs_past_the_box_on_every_side_so_it_meets_the_brush_wall() {
    let brushes = vec![cuboid(
        [-16.0, -16.0, 0.0],
        [32.0, 32.0, 8.0],
        Op::Subtract,
        0,
    )];
    // Two chunks by two, so a border is not also the far side of one chunk.
    let volume =
        Volume::new(Op::Subtract, 1, CellSize::Quarter, [0, 0, -8], [2, 2, 1]).expect("valid");
    let meshes = mesh_all(&volume, &brushes);
    let wall = meshes.iter().flat_map(|(_, m)| &m.vertices);
    let (lo, hi) = volume.bounds();
    let (mut min, mut max) = ([f64::MAX; 2], [f64::MIN; 2]);
    for q in wall.filter(|q| q.pos.z.abs() < 0.1) {
        let p = q.pos.to_f64();
        min = [min[0].min(p.x), min[1].min(p.y)];
        max = [max[0].max(p.x), max[1].max(p.y)];
    }
    assert!(min[0] < lo.x && min[1] < lo.y, "low side stops at {min:?}");
    assert!(max[0] > hi.x && max[1] > hi.y, "high side stops at {max:?}");
}

#[test]
fn a_wall_lying_on_the_ring_still_closes_behind_it() {
    // The room's ceiling is exactly where the ring's first row is.
    let brushes = vec![cuboid(
        [-16.0, -16.0, 0.0],
        [32.0, 32.0, 4.0],
        Op::Subtract,
        0,
    )];
    let volume = Volume::new(Op::Subtract, 1, CellSize::Quarter, [0; 3], [1, 1, 1]).expect("valid");
    let meshes = mesh_all(&volume, &brushes);
    let top = meshes[0]
        .1
        .vertices
        .iter()
        .map(|q| q.pos.z)
        .fold(f32::MIN, f32::max);
    let cell = volume.cell().units() as f32;
    assert!(top >= 4.0 && top < 4.0 + cell, "ceiling at {top}");
}

#[test]
fn moving_a_brush_moves_untouched_rock_and_keeps_the_sculpting() {
    let room = |z: f64| {
        vec![cuboid(
            [-16.0, -16.0, z],
            [32.0, 32.0, 8.0],
            Op::Subtract,
            0,
        )]
    };
    let brushes = room(0.0);
    let mut volume =
        Volume::new(Op::Subtract, 1, CellSize::Quarter, [0, 0, -16], [1, 1, 2]).expect("valid");
    let mut ctx = Context::new(&volume, &brushes);
    let pit_centre = Vec3d::new(2.0, 2.0, -2.0);
    let pit = Prim::sphere(pit_centre, 1.0).expect("valid");
    apply_dab(&mut volume, &mut ctx, &pit, DabOp::Carve);
    let cell = volume.cell().units() as f32;
    let survey = |bs: &[Brush]| {
        let (mut flat, mut deep) = (f32::MIN, f32::MAX);
        for (_, m) in mesh_all(&volume, bs) {
            for v in &m.vertices {
                let off = (v.pos.to_f64() - pit_centre).length();
                if off < 1.1 {
                    deep = deep.min(v.pos.z);
                }
                if off > 2.2 && v.cell.iter().all(|c| (0..15).contains(c)) {
                    flat = flat.max(v.pos.z);
                }
            }
        }
        (flat, deep)
    };
    // The pit floats below the floor at first: rock all around it is closed,
    // and the far surface is the floor.
    let (flat, deep) = survey(&brushes);
    assert_eq!(flat, 0.0);
    assert!((deep + 3.0).abs() < cell, "pit bottom {deep}");
    // Lower the floor by one unit: the flat part follows, the pit stays put.
    let (flat, deep) = survey(&room(-1.0));
    assert_eq!(flat, -1.0);
    assert!((deep + 3.0).abs() < cell, "pit bottom {deep}");
}

#[test]
fn a_later_subtract_brush_cuts_finished_rock_and_a_later_add_does_not_print_on_it() {
    let mut volume =
        Volume::new(Op::Add, 5, CellSize::Quarter, [0, 0, 0], [1, 1, 1]).expect("valid");
    let mut ctx = Context::new(&volume, &[]);
    let rock = Prim::cuboid(Vec3d::new(-1.0, -1.0, -1.0), Vec3d::new(5.0, 5.0, 5.0)).expect("ok");
    apply_dab(&mut volume, &mut ctx, &rock, DabOp::Add { material: 0 });
    let door = vec![cuboid([1.0, -1.0, 1.0], [3.0, 5.0, 3.0], Op::Subtract, 6)];
    let centre = Vec3d::new(2.0, 2.0, 2.0);
    let solid_at_centre = |brushes: &[Brush]| {
        let mut found = None;
        each_sample(&volume, brushes, |_, p, value| {
            if p == centre {
                found = Some(value >= 0);
            }
        });
        found.expect("the lattice has that point")
    };
    assert!(solid_at_centre(&[]));
    assert!(!solid_at_centre(&door), "the door cuts the rock");
    // An add brush later still is the BSP's job: the field does not follow it.
    let before = mesh_all(&volume, &door);
    let mut more = door.clone();
    more.push(cuboid([1.5, 1.5, 1.5], [2.5, 2.5, 2.5], Op::Add, 7));
    assert_eq!(before, mesh_all(&volume, &more));
}

#[test]
fn a_brush_before_an_add_volume_does_not_change_it() {
    let mut volume =
        Volume::new(Op::Add, 5, CellSize::Quarter, [0, 0, 0], [1, 1, 1]).expect("valid");
    let mut ctx = Context::new(&volume, &[]);
    let rock = Prim::sphere(Vec3d::new(2.0, 2.0, 2.0), 1.0).expect("valid");
    apply_dab(&mut volume, &mut ctx, &rock, DabOp::Add { material: 0 });
    let earlier = vec![cuboid([0.0, 0.0, 0.0], [4.0, 4.0, 1.0], Op::Add, 1)];
    assert_eq!(mesh_all(&volume, &[]), mesh_all(&volume, &earlier));
}

#[test]
fn sloped_brushes_give_chunks_the_same_shared_samples() {
    let brushes = level();
    let mut volume = subtract_volume();
    let mut ctx = Context::new(&volume, &brushes);
    let mut rng = Rng::new(9);
    let (lo, hi) = volume.bounds();
    for _ in 0..12 {
        let p = Vec3d::new(
            rng.range_f64(lo.x, hi.x),
            rng.range_f64(lo.y, hi.y),
            rng.range_f64(lo.z, hi.z),
        );
        let prim = Prim::sphere(p, rng.range_f64(0.5, 2.0)).expect("valid");
        apply_dab(&mut volume, &mut ctx, &prim, DabOp::Carve);
    }
    assert_shared_vertices_agree(&mesh_all(&volume, &brushes));
}

#[test]
fn volumes_outside_the_limits_are_refused() {
    let make = |origin, chunks| Volume::new(Op::Add, 0, CellSize::One, origin, chunks);
    assert_eq!(make([0; 3], [0, 1, 1]), Err(VolumeError::BadSize));
    assert_eq!(make([0; 3], [17, 1, 1]), Err(VolumeError::BadSize));
    assert_eq!(make([i32::MAX; 3], [1, 1, 1]), Err(VolumeError::OutOfRange));
    assert_eq!(make([i32::MIN; 3], [1, 1, 1]), Err(VolumeError::OutOfRange));
    assert!(make([-100, 0, 100], [16, 16, 16]).is_ok());
}

#[test]
fn only_a_subtract_volume_has_a_shell() {
    let add = Volume::new(Op::Add, 0, CellSize::One, [0; 3], [1, 1, 1]).expect("valid");
    assert!(add.shell().is_none());
    let sub = Volume::new(Op::Subtract, 3, CellSize::Quarter, [4, 0, 0], [1, 1, 1]).expect("valid");
    let shell = sub.shell().expect("has one").expect("valid");
    assert_eq!((shell.op(), shell.order()), (Op::Subtract, 3));
    assert!(shell.contains(Vec3d::new(2.0, 2.0, 2.0)));
    assert!(!shell.contains(Vec3d::new(0.5, 2.0, 2.0)));
}

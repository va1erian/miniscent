//! Light on voxel surfaces: what shadows what, what a cell is lit like, and how
//! the records are kept.

use std::sync::atomic::AtomicBool;

use super::bake::luminance;
use super::cells::{Direct, direct_cell};
use super::*;
use crate::brush::{Brush, Op};
use crate::bsp::Bsp;
use crate::compile;
use crate::entity::Level;
use crate::materials::ROCK;
use crate::voxel::{CellSize, Context, DabOp, Prim, Volume, VoxelWorld, apply_dab};
use mscent_math::{Vec3, Vec3d};

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn cuboid(lo: [f64; 3], hi: [f64; 3], op: Op, order: u32) -> Brush {
    Brush::cuboid(v(lo[0], lo[1], lo[2]), v(hi[0], hi[1], hi[2]), op, order).expect("valid")
}

const ROCK_ID: u8 = ROCK as u8;

/// A 16 by 16 by 8 room with a pillar of 0.25 cells standing on its floor at
/// (10, 10): a sphere of radius 1.5 whose lower part is sunk in the floor. With
/// `plate`, a slab hangs over it. With `wide`, the volume has a second chunk and
/// the sphere is centred on the border between them.
fn pillar_level(plate: bool, wide: bool) -> Level {
    let chunks = if wide { [2, 1, 1] } else { [1, 1, 1] };
    let mut volume = Volume::new(Op::Add, 1, CellSize::Quarter, [32, 32, 0], chunks).expect("ok");
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &[]);
    let x = if wide { 12.0 } else { 10.0 };
    let ball = Prim::sphere(v(x, 10.0, 1.0), 1.5).expect("sphere");
    apply_dab(
        &mut volume,
        &mut ctx,
        &ball,
        DabOp::Add { material: ROCK_ID },
    );
    let mut brushes = vec![cuboid([0.0; 3], [16.0, 16.0, 8.0], Op::Subtract, 0)];
    if plate {
        brushes.push(cuboid([7.0, 7.0, 4.5], [13.0, 13.0, 4.75], Op::Add, 2));
    }
    Level {
        brushes,
        volumes: vec![volume],
        ..Default::default()
    }
}

fn world_of(level: &Level) -> (Bsp, VoxelWorld) {
    let bsp = compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(level, &level.static_brushes(), &bsp);
    (bsp, world)
}

fn cfg() -> BakeConfig {
    BakeConfig {
        workers: 2,
        luxel_size: 0.5,
        ambient: Vec3::ZERO,
        light_radius: 0.0,
        shadow_samples: 1,
        ..BakeConfig::default()
    }
}

fn lamp(x: f32, y: f32, z: f32) -> PointLight {
    PointLight {
        position: v3(x, y, z),
        color: v3(1.0, 1.0, 1.0),
        radius: 40.0,
    }
}

fn floor() -> LightFace {
    LightFace {
        vertices: vec![
            v3(0.0, 0.0, 0.0),
            v3(16.0, 0.0, 0.0),
            v3(16.0, 16.0, 0.0),
            v3(0.0, 16.0, 0.0),
        ],
        normal: v3(0.0, 0.0, 1.0),
        density: Default::default(),
    }
}

/// Luminance of the floor luxel nearest `(x, y)`.
fn floor_at(map: &Lightmap, x: f32, y: f32) -> f32 {
    let l = map.sample_world(0, v3(x, y, 0.0)).expect("on the floor");
    luminance(l.color)
}

/// The site with the highest position.
fn top_site(sites: &CellSites) -> usize {
    (0..sites.len())
        .max_by(|&a, &b| sites.pos(a).z.total_cmp(&sites.pos(b).z))
        .expect("the pillar has a surface")
}

#[test]
fn a_ray_through_a_pillar_is_blocked_only_when_the_volume_is_an_occluder() {
    let (bsp, world) = world_of(&pillar_level(false, false));
    let (brushes, both) = (Occluders::brushes(&bsp), Occluders::new(&bsp, &world));
    assert!(!brushes.has_volumes() && both.has_volumes());
    let through = (v(6.0, 10.0, 1.0), v(14.0, 10.0, 1.0));
    assert!(brushes.segment_clear(through.0, through.1));
    assert!(!both.segment_clear(through.0, through.1));
    // Beside it and over it, the same occluders let the ray pass.
    assert!(both.segment_clear(v(6.0, 14.0, 1.0), v(14.0, 14.0, 1.0)));
    assert!(both.segment_clear(v(6.0, 10.0, 3.5), v(14.0, 10.0, 3.5)));
    assert!(!both.segment_clear(v(f64::NAN, 0.0, 0.0), v(1.0, 1.0, 1.0)));
}

#[test]
fn a_voxel_pillar_shadows_the_floor_and_without_volumes_the_bake_is_what_it_was() {
    let (bsp, world) = world_of(&pillar_level(false, false));
    let (faces, lights) = ([floor()], [lamp(10.0, 10.0, 7.0)]);
    let sites = CellSites::of(&world);
    let with = bake_in(
        &Occluders::new(&bsp, &world),
        &sites,
        &faces,
        &lights,
        &cfg(),
        &AtomicBool::new(false),
    )
    .expect("bakes");
    let without = bake(&bsp, &faces, &lights, &cfg()).expect("bakes");
    // Under the shadow of the pillar's top, and well clear of it.
    assert!(
        floor_at(&without, 10.8, 10.0) > 0.5,
        "lit without the pillar"
    );
    assert!(floor_at(&with, 10.8, 10.0) < 0.01, "in its shadow with it");
    let clear = floor_at(&with, 13.5, 10.0);
    assert!(
        (clear - floor_at(&without, 13.5, 10.0)).abs() < 1e-5,
        "{clear}"
    );
    // Brush-only occluders and no cells are the bake that existed before.
    let none = bake_in(
        &Occluders::brushes(&bsp),
        &CellSites::default(),
        &faces,
        &lights,
        &cfg(),
        &AtomicBool::new(false),
    )
    .expect("bakes");
    assert_eq!(none.to_bytes(7), without.to_bytes(7));
    assert!(without.cells().is_empty());
}

#[test]
fn a_hanging_plate_shadows_the_top_of_a_voxel_pillar() {
    let mut tops = Vec::new();
    for plate in [false, true] {
        let (bsp, world) = world_of(&pillar_level(plate, false));
        let sites = CellSites::of(&world);
        let occ = Occluders::new(&bsp, &world);
        let lights = [lamp(10.0, 10.0, 7.0)];
        let env = CellLighting {
            occ: &occ,
            lights: &lights,
            cfg: &cfg(),
        };
        let top = top_site(&sites);
        let records = light_cells(&env, &sites, &[top]);
        tops.push(luminance(records[0].color()));
    }
    assert!(tops[0] > 0.5, "lit from above: {}", tops[0]);
    assert!(tops[1] < 0.01, "under the plate: {}", tops[1]);
}

#[test]
fn the_root_of_a_pillar_is_lit_where_its_normal_leans_into_the_floor() {
    let (bsp, world) = world_of(&pillar_level(false, false));
    let sites = CellSites::of(&world);
    let occ = Occluders::new(&bsp, &world);
    let lights = [lamp(3.0, 10.0, 1.5)];
    let env = CellLighting {
        occ: &occ,
        lights: &lights,
        cfg: &cfg(),
    };
    let all: Vec<usize> = (0..sites.len()).collect();
    let records = light_cells(&env, &sites, &all);
    // Cells on the side the lamp faces: the root, and the middle of the ball.
    let facing = |i: usize| sites.pos(i).x < 9.5 && (sites.pos(i).y - 10.0).abs() < 0.6;
    let root: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| facing(i) && sites.pos(i).z < 0.2)
        .collect();
    let middle: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| facing(i) && (0.8..1.2).contains(&sites.pos(i).z))
        .collect();
    assert!(!root.is_empty() && !middle.is_empty());
    let mean = |ids: &[usize]| {
        ids.iter()
            .map(|&i| luminance(records[i].color()))
            .sum::<f32>()
            / ids.len() as f32
    };
    let starved = root
        .iter()
        .filter(|&&i| direct_cell(&env, &sites, i) == Direct::Solid)
        .count();
    assert!(
        starved > 0,
        "the case is not exercised: no root cell is in the floor"
    );
    let darkest = root
        .iter()
        .map(|&i| luminance(records[i].color()))
        .fold(f32::MAX, f32::min);
    assert!(
        darkest > 0.5 * mean(&middle),
        "root {darkest} against the middle {}",
        mean(&middle)
    );
}

#[test]
fn a_chunk_lit_alone_gets_the_records_the_whole_bake_gives_it() {
    let (bsp, world) = world_of(&pillar_level(false, true));
    let sites = CellSites::of(&world);
    assert!(world.chunks.len() >= 2, "the ball spans two chunks");
    let lights = [lamp(10.0, 10.0, 7.0)];
    // No bounce: the records are what `light_cells` is for.
    let flat = BakeConfig {
        albedo: 0.0,
        ..cfg()
    };
    let occ = Occluders::new(&bsp, &world);
    let map = bake_in(
        &occ,
        &sites,
        &[floor()],
        &lights,
        &flat,
        &AtomicBool::new(false),
    )
    .expect("bakes");
    let env = CellLighting {
        occ: &occ,
        lights: &lights,
        cfg: &flat,
    };
    for chunk in 0..world.chunks.len() {
        let ids = sites.of_chunk(&world, chunk);
        assert!(!ids.is_empty());
        let alone = light_cells(&env, &sites, &ids);
        for (&i, r) in ids.iter().zip(alone) {
            assert_eq!(r, map.cells()[i], "site {i} of chunk {chunk}");
        }
    }
}

#[test]
fn neighbouring_chunks_read_the_same_record_for_a_border_cell() {
    let (_, world) = world_of(&pillar_level(false, true));
    let sites = CellSites::of(&world);
    // The volume is two chunks along x, so a chunk's place in the table is its x.
    let global = |chunk: usize, cell: [i8; 3]| {
        let at = world.volumes[0]
            .table
            .iter()
            .position(|&c| c as usize == chunk);
        let x = at.expect("in the table") as i32 * 16 + i32::from(cell[0]);
        [x, i32::from(cell[1]), i32::from(cell[2])]
    };
    let mut shared = 0;
    for (a, ca) in world.chunks.iter().enumerate() {
        for (b, cb) in world.chunks.iter().enumerate().skip(a + 1) {
            for va in &ca.lods[0].vertices {
                for vb in &cb.lods[0].vertices {
                    if global(a, va.cell) != global(b, vb.cell) {
                        continue;
                    }
                    let (ia, ib) = (sites.lookup(a, va.cell), sites.lookup(b, vb.cell));
                    assert!(ia.is_some() && ia == ib, "{ia:?} against {ib:?}");
                    assert_eq!(va.pos, vb.pos, "one cell, one place");
                    shared += 1;
                }
            }
        }
    }
    assert!(shared > 8, "{shared} cells meet at the chunk border");
}

#[test]
fn painting_a_volume_keeps_the_shape_key_and_carving_it_changes_it() {
    let level = pillar_level(false, false);
    let key = |level: &Level| shape_key(&world_of(level).1);
    let edit = |op: DabOp| {
        let mut level = level.clone();
        let shape = level.volumes[0].clone();
        let mut ctx = Context::new(&shape, &[]);
        let ball = Prim::sphere(v(10.0, 10.0, 1.0), 1.0).expect("sphere");
        apply_dab(&mut level.volumes[0], &mut ctx, &ball, op);
        level
    };
    let before = key(&level);
    assert_eq!(before, key(&edit(DabOp::Paint { material: 3 })));
    assert_ne!(before, key(&edit(DabOp::Carve)));
}

#[test]
fn records_round_trip_through_the_file_and_forged_ones_are_refused() {
    let (bsp, world) = world_of(&pillar_level(false, false));
    let sites = CellSites::of(&world);
    let (faces, lights) = ([floor()], [lamp(10.0, 10.0, 7.0)]);
    let occ = Occluders::new(&bsp, &world);
    let map = bake_in(
        &occ,
        &sites,
        &faces,
        &lights,
        &cfg(),
        &AtomicBool::new(false),
    )
    .expect("bakes");
    assert_eq!(map.cells().len(), sites.len());
    let bytes = map.to_bytes(9);
    let back = Lightmap::from_bytes(&bytes, 9).expect("reads");
    assert_eq!(back.cells(), map.cells());
    let reseal = |mut b: Vec<u8>| {
        let n = b.len() - 8;
        let mut h = store::FNV_BASIS;
        for &x in &b[..n] {
            h = (h ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        b[n..].copy_from_slice(&h.to_le_bytes());
        b
    };
    // The count says more or fewer records than there are bytes for.
    let count_at = bytes.len() - 8 - map.cells().len() * RECORD_BYTES - 4;
    for lie in [map.cells().len() as u32 + 1, 0, u32::MAX] {
        let mut b = bytes.clone();
        b[count_at..count_at + 4].copy_from_slice(&lie.to_le_bytes());
        assert_eq!(
            Lightmap::from_bytes(&reseal(b), 9).err(),
            Some(store::StoreError::Corrupt),
            "a count of {lie}"
        );
    }
    // Any bytes at all are a record, and never an unusable number.
    let mut b = bytes.clone();
    for (i, x) in b[count_at + 4..bytes.len() - 8].iter_mut().enumerate() {
        *x = (i as u8).wrapping_mul(97).wrapping_add(0x80);
    }
    let forged = Lightmap::from_bytes(&reseal(b), 9).expect("reads");
    for r in forged.cells() {
        assert!(r.color().is_finite() && r.dir().is_finite());
        assert!(r.dir().length() < 1.75);
    }
}

#[test]
fn a_cell_whose_lifted_point_is_in_its_own_rock_takes_light_from_around_it() {
    let level = pillar_level(false, false);
    let (bsp, world) = world_of(&level);
    let occ = Occluders::new(&bsp, &world);
    let inside = world
        .chunks
        .iter()
        .flat_map(|c| c.lods[0].vertices.iter())
        .map(|v| v.pos - v.normal * 0.2)
        .find(|p| world.is_solid(p.to_f64()))
        .expect("a point in rock");
    assert!(occ.in_solid(inside.to_f64()));
    assert!(!Occluders::brushes(&bsp).in_solid(inside.to_f64()));
}

/// A flat voxel slab `half` either side of z = 0, and its sites.
fn slab_sites(half: f64) -> CellSites {
    use crate::voxel::{CellSize, Context, DabOp, Prim, Volume, apply_dab};
    let mut volume =
        Volume::new(Op::Add, 0, CellSize::Quarter, [-16, -16, -16], [2, 2, 2]).expect("valid");
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &[]);
    let slab = Prim::cuboid(v(-3.0, -3.0, -half), v(3.0, 3.0, half)).expect("valid");
    let m = DabOp::Add { material: ROCK_ID };
    apply_dab(&mut volume, &mut ctx, &slab, m);
    // A room around it, or the BSP's solid would bury every triangle.
    let level = Level {
        brushes: vec![cuboid([-9.0; 3], [9.0; 3], Op::Subtract, 0)],
        volumes: vec![volume.with_order(1)],
        ..Default::default()
    };
    let (_, world) = world_of(&level);
    CellSites::of(&world)
}

fn top_near_edge(sites: &CellSites) -> usize {
    (0..sites.len())
        .filter(|&i| sites.normal(i).z > 0.9 && sites.pos(i).y.abs() < 1.0)
        .min_by(|&a, &b| sites.pos(a).x.abs().total_cmp(&sites.pos(b).x.abs()))
        .expect("a cell on top")
}

#[test]
fn a_hard_shadow_edge_is_read_as_a_ramp_across_cells() {
    let sites = slab_sites(0.4);
    let i = top_near_edge(&sites);
    let at = sites.pos(i).x;
    // Lit on one side of the cell, dark on the other.
    let known = |j: usize| Direct::Lit {
        color: if sites.pos(j).x <= at {
            v3(1.0, 1.0, 1.0)
        } else {
            Vec3::ZERO
        },
        dir: v3(0.0, 0.0, 1.0),
    };
    let (color, _) = super::cells::filtered(&sites, i, known(i), &known);
    assert!(
        color.x > 0.3 && color.x < 0.9,
        "the edge cell shows {}",
        color.x
    );
}

#[test]
fn light_is_not_read_from_the_far_side_of_a_thin_shape() {
    // Thinner than a cell, so the two sides are neighbouring cells.
    let sites = slab_sites(0.1);
    let i = (0..sites.len())
        .find(|&i| sites.normal(i).z < -0.9 && sites.pos(i).x.abs() < 1.0)
        .expect("a cell underneath");
    // Only the top is lit, and the top is within reach of the bottom.
    let known = |j: usize| Direct::Lit {
        color: if sites.normal(j).z > 0.0 {
            v3(1.0, 1.0, 1.0)
        } else {
            Vec3::ZERO
        },
        dir: v3(0.0, 0.0, 1.0),
    };
    let (color, _) = super::cells::filtered(&sites, i, known(i), &known);
    assert_eq!(color.x, 0.0);
}

//! The cell index and the point and box queries collision relies on.

use std::sync::Arc;

use mscent_math::{Aabb, Vec3, Vec3d};

use super::block::block_index;
use super::test_util::coords;
use super::{
    CHUNK_EDGE, CellIndex, CellSize, ChunkMesh, Context, DabOp, Mesher, NO_CHUNK, Prim, Volume,
    VolumeGeometry, VoxelChunk, VoxelWorld, apply_dab,
};
use crate::brush::{Brush, Op};
use crate::compile;
use crate::entity::Level;
use crate::materials::ROCK;
use crate::voxel::{AirMask, VoxelVertex};

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

/// A room whose +Y wall a volume straddles, with a pocket carved into the rock.
fn carved() -> (Level, VoxelWorld) {
    let room = Brush::cuboid(v(-8.0, -8.0, 0.0), v(8.0, 0.0, 6.0), Op::Subtract, 0)
        .expect("valid")
        .with_material(ROCK);
    let brushes = vec![room];
    let mut volume =
        Volume::new(Op::Subtract, 1, CellSize::Quarter, [-16, -16, 0], [2, 2, 1]).expect("valid");
    let mut ctx = Context::new(&volume, &brushes);
    let pocket = Prim::sphere(v(0.0, 2.0, 2.0), 1.0).expect("valid");
    apply_dab(&mut volume, &mut ctx, &pocket, DabOp::Carve);
    let level = Level {
        brushes,
        volumes: vec![volume],
        ..Default::default()
    };
    let bsp = compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    (level, world)
}

#[test]
fn solid_agrees_with_the_field_away_from_the_surface() {
    let (level, world) = carved();
    let bsp = compile(&level.csg_brushes()).expect("compiles");
    let volume = &level.volumes[0];
    let brushes = level.static_brushes();
    let mut ctx = Context::in_level(volume, &brushes, &level.volumes);
    let mut mesher = Mesher::new();
    let (mut checked, mut rock) = (0, 0);
    for coord in coords(volume) {
        mesher.mesh(volume, &mut ctx, coord).expect("chunk exists");
        let first = coord.map(|c| i32::from(c) * CHUNK_EDGE as i32);
        for z in 0..CHUNK_EDGE as i32 {
            for y in 0..CHUNK_EDGE as i32 {
                for x in 0..CHUNK_EDGE as i32 {
                    let density = mesher.block().density[block_index(x, y, z)];
                    let p = volume
                        .lattice
                        .position([first[0] + x, first[1] + y, first[2] + z]);
                    // Near the surface the mesh and the samples may differ by
                    // a fraction of a cell; the BSP decides where it is solid.
                    if density.abs() < 8 || bsp.is_solid(p) {
                        continue;
                    }
                    checked += 1;
                    rock += usize::from(density >= 0);
                    assert_eq!(world.is_solid(p), density >= 0, "{p:?} density {density}");
                }
            }
        }
    }
    assert!(
        checked > 5000 && rock > 500,
        "{checked} samples, {rock} rock"
    );
}

#[test]
fn the_pocket_is_open_and_the_rock_around_it_is_not() {
    let (_, world) = carved();
    assert!(!world.is_solid(v(0.0, 2.0, 2.0)), "inside the pocket");
    assert!(world.is_solid(v(0.0, 3.5, 2.0)), "rock beyond the pocket");
    assert!(world.is_solid(v(0.0, 2.0, 0.5)), "rock below it");
    assert!(!world.is_solid(v(0.0, -3.0, 2.0)), "the room");
    assert!(!world.is_solid(v(50.0, 50.0, 50.0)), "outside every volume");
    assert!(!world.is_solid(v(f64::NAN, 0.0, 0.0)), "not a number");
}

/// Far from any surface only the chunk's recorded verdict can say.
#[test]
fn deep_inside_rock_is_solid_with_no_surface_near() {
    let room = Brush::cuboid(v(-8.0, -8.0, 0.0), v(8.0, 0.0, 6.0), Op::Subtract, 0).expect("valid");
    let volume =
        Volume::new(Op::Subtract, 1, CellSize::Half, [-16, -16, 0], [2, 8, 1]).expect("valid");
    let level = Level {
        brushes: vec![room],
        volumes: vec![volume],
        ..Default::default()
    };
    let bsp = compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    // Six chunks of 8 units from the wall: beyond the search.
    let deep = v(0.0, 50.0, 4.0);
    assert!(!bsp.is_solid(deep), "the volume's box is open in the BSP");
    assert!(world.is_solid(deep), "but its rock is solid");
    assert!(world.is_solid(v(2.0, 30.0, 3.0)), "and nearer");
    assert!(!world.is_solid(v(0.0, -4.0, 3.0)), "the room is open");
}

#[test]
fn the_answer_is_the_same_on_every_ask() {
    let (_, a) = carved();
    let (_, b) = carved();
    for i in 0..400 {
        let t = f64::from(i);
        let p = v(
            (t * 0.37) % 8.0 - 4.0,
            (t * 0.71) % 8.0 - 4.0,
            (t * 0.13) % 4.0,
        );
        assert_eq!(a.is_solid(p), b.is_solid(p), "{p:?}");
        assert_eq!(a.is_solid(p), a.is_solid(p), "{p:?}");
    }
}

/// Triangles of `chunk` touching the box, by brute force.
fn brute(mesh: &ChunkMesh, lo: Vec3, hi: Vec3) -> usize {
    let query = Aabb::new(lo, hi);
    mesh.triangles
        .iter()
        .filter(|t| {
            let b = t.iter().fold(Aabb::empty(), |b, &i| {
                b.including(mesh.vertices[usize::from(i)].pos)
            });
            b.intersects(&query)
        })
        .count()
}

#[test]
fn a_box_query_finds_each_triangle_it_touches_once() {
    let (_, world) = carved();
    let chunk = world
        .chunks
        .iter()
        .max_by_key(|c| c.lods[0].triangles.len())
        .expect("a chunk");
    let mesh = &chunk.lods[0];
    assert!(mesh.triangles.len() > 100, "{}", mesh.triangles.len());
    let (b0, b1) = (mesh.bounds.min, mesh.bounds.max);
    let boxes = [
        (b0, b1),
        (b0, b0 + Vec3::new(0.5, 0.5, 0.5)),
        (b1 - Vec3::new(0.7, 0.2, 0.9), b1),
        (b0.lerp(b1, 0.4), b0.lerp(b1, 0.6)),
        (b0 - Vec3::new(5.0, 5.0, 5.0), b0 - Vec3::new(4.0, 4.0, 4.0)),
    ];
    for (lo, hi) in boxes {
        let mut seen = 0;
        let mut ids = Vec::new();
        chunk.triangles_in(lo, hi, |tri| {
            seen += 1;
            ids.push(tri.map(|x| [x.pos.x, x.pos.y, x.pos.z].map(f32::to_bits)));
        });
        let exact = brute(mesh, lo, hi);
        assert!(seen >= exact, "{seen} found of {exact} touching: lost some");
        // Whole cells are visited, so a few more than touch is fine.
        assert!(seen <= mesh.triangles.len());
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "a triangle came up twice");
    }
    let mut all = 0;
    chunk.triangles_in(b0, b1, |_| all += 1);
    assert_eq!(
        all,
        mesh.triangles.len(),
        "the whole box finds every triangle once"
    );
    let mut far = 0;
    chunk.triangles_in(
        b0 - Vec3::new(9.0, 9.0, 9.0),
        b0 - Vec3::new(8.0, 8.0, 8.0),
        |_| far += 1,
    );
    assert_eq!(far, 0, "a box clear of the mesh finds nothing");
}

#[test]
fn the_index_is_small_next_to_the_mesh() {
    let (_, world) = carved();
    for chunk in &world.chunks {
        let mesh = &chunk.lods[0];
        let index = &chunk.index;
        assert_eq!(index.triangles() as usize, mesh.triangles.len());
        let budget = 4 * (513 + 16 * mesh.triangles.len());
        assert!(index.bytes() <= budget, "{} of {budget}", index.bytes());
    }
}

fn vertex(x: f32, y: f32, z: f32) -> VoxelVertex {
    VoxelVertex {
        pos: Vec3::new(x, y, z),
        normal: Vec3::new(0.0, 0.0, 1.0),
        cell: [0; 3],
    }
}

fn mesh_of(vertices: Vec<VoxelVertex>, triangles: Vec<[u16; 3]>) -> ChunkMesh {
    let bounds = vertices
        .iter()
        .fold(Aabb::empty(), |b, v| b.including(v.pos));
    ChunkMesh {
        materials: vec![0; triangles.len()],
        vertices,
        triangles,
        bounds,
        air: AirMask::NONE,
    }
}

#[test]
fn a_hostile_mesh_is_indexed_without_panic_and_within_its_budget() {
    let nan = f32::NAN;
    let meshes = [
        mesh_of(Vec::new(), Vec::new()),
        // Triangles naming vertices that do not exist.
        mesh_of(vec![vertex(0.0, 0.0, 0.0)], vec![[0, 1, 2], [u16::MAX; 3]]),
        // Not a number, infinite and enormous coordinates.
        mesh_of(
            vec![
                vertex(nan, 0.0, 0.0),
                vertex(f32::INFINITY, 1.0, 1.0),
                vertex(1.0e30, 0.0, 0.0),
            ],
            vec![[0, 1, 2]],
        ),
        // One triangle across the whole grid, which the budget refuses.
        mesh_of(
            vec![
                vertex(0.0, 0.0, 0.0),
                vertex(100.0, 0.0, 100.0),
                vertex(0.0, 100.0, 100.0),
            ],
            vec![[0, 1, 2]],
        ),
    ];
    for mesh in meshes {
        let index = CellIndex::build(&mesh);
        assert!(index.bytes() <= 4 * (513 + 16 * mesh.triangles.len()));
        let chunk = VoxelChunk {
            lods: [mesh.clone(), mesh.clone(), mesh],
            leaves: Vec::new(),
            index,
        };
        let mut n = 0;
        let boxes = [
            (Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0)),
            (Vec3::new(nan, nan, nan), Vec3::new(nan, nan, nan)),
            (
                Vec3::new(1.0e30, 0.0, 0.0),
                Vec3::new(f32::INFINITY, 1.0, 1.0),
            ),
        ];
        for (lo, hi) in boxes {
            chunk.triangles_in(lo, hi, |_| n += 1);
        }
        assert!(n <= 3 * chunk.lods[0].triangles.len());
    }
}

#[test]
fn a_hostile_world_answers_without_panic() {
    let mesh = mesh_of(
        vec![
            vertex(0.0, 0.0, 0.0),
            vertex(1.0, 0.0, 0.0),
            vertex(0.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2]],
    );
    let chunk = Arc::new(VoxelChunk {
        index: CellIndex::build(&mesh),
        lods: [mesh.clone(), mesh.clone(), mesh],
        leaves: Vec::new(),
    });
    let geometry = |size: f32, table: Vec<u32>, rock: Vec<bool>| VolumeGeometry {
        chunks: [2, 1, 1],
        air: vec![AirMask::NONE; 2],
        table,
        origin: Vec3::ZERO,
        chunk_size: size,
        rock,
    };
    let shapes = [
        geometry(0.0, vec![0, NO_CHUNK], vec![true, true]),
        geometry(f32::NAN, vec![0, 0], vec![true, true]),
        geometry(f32::INFINITY, vec![0, 0], vec![true, true]),
        geometry(-4.0, vec![0, 0], vec![false, false]),
        // A table naming a chunk that does not exist, and one that is short.
        geometry(4.0, vec![9, NO_CHUNK], vec![true, true]),
        geometry(4.0, vec![0], Vec::new()),
        geometry(4.0, vec![0, 0], vec![true]),
    ];
    for volume in shapes {
        let world = VoxelWorld {
            chunks: vec![chunk.clone()],
            volumes: vec![volume],
        };
        let points = [
            v(0.2, 0.2, 0.0),
            v(5.0, 0.5, 0.5),
            v(f64::NAN, 0.0, 0.0),
            v(1.0e300, 0.0, 0.0),
        ];
        for p in points {
            world.is_solid(p);
        }
        let (lo, hi) = (Vec3::new(-1.0, -1.0, -1.0), Vec3::new(9.0, 9.0, 9.0));
        world.for_each_chunk_in(lo, hi, |c| c.triangles_in(lo, hi, |_| {}));
        let nan = Vec3::new(f32::NAN, 0.0, 0.0);
        world.for_each_chunk_in(nan, nan, |_| unreachable!("a box with no extent"));
    }
    let empty = VoxelWorld::default();
    assert!(!empty.is_solid(v(0.0, 0.0, 0.0)));
}

#[test]
fn the_nearest_surface_search_finds_what_a_full_scan_finds() {
    let room =
        Brush::cuboid(v(-20.0, -20.0, -20.0), v(20.0, 20.0, 20.0), Op::Subtract, 0).expect("valid");
    let brushes = vec![room];
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Quarter, [-16, -16, -16], [2, 2, 2]).expect("valid");
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &brushes);
    for (x, y, z) in [(-1.5, -1.0, 0.5), (1.5, 1.0, -0.5), (0.0, 2.0, 1.5)] {
        let ball = Prim::sphere(v(x, y, z), 1.2).expect("valid");
        apply_dab(&mut volume, &mut ctx, &ball, DabOp::Add { material: 1 });
    }
    let level = Level {
        brushes,
        volumes: vec![volume],
        ..Default::default()
    };
    let bsp = compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    let triangles: Vec<[Vec3d; 3]> = (world.chunks.iter())
        .flat_map(|c| {
            let m = &c.lods[0];
            m.triangles
                .iter()
                .map(|t| t.map(|i| m.vertices[usize::from(i)].pos.to_f64()))
        })
        .collect();
    let g = &world.volumes[0];
    let size = f64::from(g.chunk_size);
    let mut rng = mscent_math::rng::Rng::new(7);
    for _ in 0..3000 {
        let p = v(
            rng.range_f64(-3.9, 3.9),
            rng.range_f64(-3.9, 3.9),
            rng.range_f64(-3.9, 3.9),
        );
        let origin = g.origin.to_f64();
        let home =
            [p.x - origin.x, p.y - origin.y, p.z - origin.z].map(|d| (d / size).floor() as i32);
        let found = world.nearest_surface(g, p, home, size).expect("a surface");
        let best = (triangles.iter())
            .map(|t| (p - super::query::closest_on_triangle(p, *t).0).length_squared())
            .fold(f64::INFINITY, f64::min);
        assert!(
            (found.d2 - best).abs() < 1e-9,
            "at {p:?}: {} against {best}",
            found.d2
        );
    }
}

/// Surface nets folds four cells onto one point at a pole of a shape, which makes
/// triangles with no area. They are the nearest surface to points on that axis,
/// and must still say which side those points are on.
#[test]
fn a_point_facing_a_pole_of_a_shape_is_told_the_right_side() {
    let room =
        Brush::cuboid(v(-20.0, -20.0, -20.0), v(20.0, 20.0, 20.0), Op::Subtract, 0).expect("valid");
    let brushes = vec![room];
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Half, [10, -8, -8], [1, 1, 1]).expect("valid");
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &brushes);
    let ball = Prim::sphere(v(9.0, 0.0, 0.0), 3.0).expect("valid");
    apply_dab(&mut volume, &mut ctx, &ball, DabOp::Add { material: 1 });
    let level = Level {
        brushes,
        volumes: vec![volume],
        ..Default::default()
    };
    let bsp = compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    for (y, z) in [(0.0, 0.0), (0.01, 0.0), (0.0, 0.01)] {
        for i in 0..18 {
            let x = 5.05 + f64::from(i) * 0.05;
            assert!(
                !world.is_solid(v(x, y, z)),
                "air at x = {x}, y = {y}, z = {z}"
            );
        }
        for x in [6.5, 9.0, 11.5] {
            assert!(
                world.is_solid(v(x, y, z)),
                "rock at x = {x}, y = {y}, z = {z}"
            );
        }
    }
}

//! M11.6: collision with voxel volumes, read from real sweeps and slides.
//!
//! The scenes are a room of brush walls with a `Subtract` volume across part of
//! its floor or east wall, so a sphere moves from brush surface to voxel
//! surface and back.

use std::sync::Arc;

use mscent_collide::{STEP_DT, Solids, slide_move, sweep_sphere};
use mscent_math::rng::Rng;
use mscent_math::{Aabb, Vec3, Vec3d};
use mscent_world::voxel::{
    AirMask, CellIndex, CellSize, ChunkMesh, Context, DabOp, NO_CHUNK, Prim, Volume,
    VolumeGeometry, VoxelChunk, VoxelVertex, VoxelWorld, apply_dab,
};
use mscent_world::{Brush, Bsp, Level, Op, compile};

const RADIUS: f32 = 0.5;
/// What a slide may lose to rounding in one step of a 10 unit/s walk.
const STEP_SLACK: f32 = 1.0e-4;

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

struct Scene {
    bsp: Bsp,
    world: VoxelWorld,
}

impl Scene {
    fn solids(&self) -> Solids<'_> {
        Solids::from(&self.bsp).with_voxels(&self.world)
    }

    fn brush_only(&self) -> Solids<'_> {
        Solids::from(&self.bsp)
    }
}

fn build(brushes: Vec<Brush>, volumes: Vec<Volume>) -> Scene {
    let level = Level {
        brushes,
        volumes,
        ..Default::default()
    };
    let bsp = compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    Scene { bsp, world }
}

fn room() -> Brush {
    Brush::cuboid(v(-12.0, -6.0, 0.0), v(12.0, 6.0, 6.0), Op::Subtract, 0).expect("valid")
}

/// The room, with a volume across its floor from x = 0 to 16 and an east wall
/// that lies inside it at x = 12.
fn floor_scene() -> Scene {
    let volume =
        Volume::new(Op::Subtract, 1, CellSize::Half, [0, -8, -8], [2, 1, 1]).expect("valid");
    build(vec![room()], vec![volume])
}

/// The room, with a volume covering the wall at x = 12 for y in -8..8 and a
/// tunnel through the wall at y = 0.
fn tunnel_scene() -> Scene {
    let brushes = vec![room()];
    let mut volume =
        Volume::new(Op::Subtract, 1, CellSize::Half, [16, -16, -8], [2, 2, 1]).expect("valid");
    let mut ctx = Context::new(&volume, &brushes);
    let tunnel = Prim::round_cone(v(11.0, 0.0, 2.0), v(20.0, 0.0, 2.0), 1.5, 1.5).expect("valid");
    apply_dab(&mut volume, &mut ctx, &tunnel, DabOp::Carve);
    build(brushes, vec![volume])
}

/// Steps a slide with `vel` set afresh each step, as a walking player does.
fn walk(solids: Solids, mut pos: Vec3, vel: Vec3, steps: usize) -> Vec<Vec3> {
    let mut path = vec![pos];
    for _ in 0..steps {
        pos = slide_move(solids, pos, vel, STEP_DT, RADIUS).pos;
        path.push(pos);
    }
    path
}

#[test]
fn a_slide_from_a_brush_floor_onto_voxel_rock_keeps_its_speed() {
    let scene = floor_scene();
    let start = Vec3::new(-6.0, 0.0, RADIUS + 0.002);
    // Pressed into the floor all the way, as gravity would.
    let vel = Vec3::new(10.0, 0.0, -1.0);
    let path = walk(scene.solids(), start, vel, 160);
    let crossed = path.iter().filter(|p| p.x > 1.0).count();
    assert!(crossed > 30, "the walk reached the voxel floor: {crossed}");
    for pair in path.windows(2) {
        let dx = pair[1].x - pair[0].x;
        assert!(
            dx >= 10.0 * STEP_DT - STEP_SLACK,
            "lost speed at x {}: {dx}",
            pair[0].x
        );
        assert!(
            (pair[1].z - RADIUS).abs() < 0.02,
            "left the floor at x {}",
            pair[1].x
        );
    }
    // Without the voxels the same walk drops through where the BSP is open.
    let bare = walk(scene.brush_only(), start, vel, 160);
    assert!(bare.last().is_some_and(|p| p.z < 0.0), "{:?}", bare.last());
}

#[test]
fn a_slide_along_a_brush_wall_through_a_volume_keeps_its_speed() {
    let scene = tunnel_scene();
    // Hugging the east wall, pushing into it, from y = -5.5 to 5.5. The volume
    // covers the middle of that.
    let start = Vec3::new(12.0 - RADIUS - 0.001, -5.5, 4.5);
    let vel = Vec3::new(5.0, 10.0, 0.0);
    let path = walk(scene.solids(), start, vel, 130);
    assert!(path.last().is_some_and(|p| p.y > 5.0));
    // The tunnel is a hole in this wall at y = 0, z = 2; z = 4.5 passes above it.
    for pair in path.windows(2) {
        let dy = pair[1].y - pair[0].y;
        assert!(
            dy >= 10.0 * STEP_DT - STEP_SLACK,
            "lost speed at y {}: {dy}",
            pair[0].y
        );
        assert!(
            pair[1].x < 12.0 - RADIUS + 0.04,
            "went into the wall at y {}",
            pair[1].y
        );
    }
}

#[test]
fn a_carved_tunnel_can_be_walked_and_uncarved_rock_cannot() {
    let scene = tunnel_scene();
    let open = walk(
        scene.solids(),
        Vec3::new(9.0, 0.0, 2.0),
        Vec3::new(10.0, 0.0, 0.0),
        120,
    );
    let end = open.last().expect("a path");
    assert!(end.x > 17.0, "through the wall by the tunnel: {end:?}");
    let blind = walk(
        scene.solids(),
        Vec3::new(9.0, 3.5, 2.0),
        Vec3::new(10.0, 0.0, 0.0),
        120,
    );
    let stopped = blind.last().expect("a path");
    assert!(
        stopped.x < 11.6 && stopped.x > 11.0,
        "stopped at the rock: {stopped:?}"
    );
    // Counting only the BSP, the volume's box is open and the sphere walks on.
    let bare = walk(
        scene.brush_only(),
        Vec3::new(9.0, 3.5, 2.0),
        Vec3::new(10.0, 0.0, 0.0),
        120,
    );
    assert!(bare.last().is_some_and(|p| p.x > 17.0));

    let solids = scene.solids();
    assert!(!solids.is_solid(v(15.0, 0.0, 2.0)), "inside the tunnel");
    assert!(solids.is_solid(v(15.0, 3.5, 2.0)), "in the rock beside it");
    assert!(
        solids.is_solid(v(30.0, 0.0, 2.0)),
        "in the BSP's rock past the box"
    );
    let t = sweep_sphere(
        solids,
        Vec3::new(9.0, 3.5, 2.0),
        Vec3::new(18.0, 3.5, 2.0),
        RADIUS,
    );
    assert!(t.hit && (t.normal.x + 1.0).abs() < 1.0e-3, "{t:?}");
}

#[test]
fn a_sphere_embedded_in_a_voxel_surface_can_leave_it_and_cannot_dig_in() {
    let scene = floor_scene();
    let sunk = Vec3::new(8.0, 0.0, RADIUS - 0.02);
    let up = sweep_sphere(
        scene.solids(),
        sunk,
        sunk + Vec3::new(0.0, 0.0, 2.0),
        RADIUS,
    );
    assert!(!up.hit && !up.start_solid, "free to rise: {up:?}");
    let down = sweep_sphere(
        scene.solids(),
        sunk,
        sunk - Vec3::new(0.0, 0.0, 2.0),
        RADIUS,
    );
    assert!(down.hit && down.fraction == 0.0, "cannot sink: {down:?}");
    assert!((down.normal.z - 1.0).abs() < 1.0e-3);
    // A centre inside the rock itself is not trapped either: which side of the
    // surface it is on is a guess, and a body must never be frozen by one.
    let inside = sweep_sphere(
        scene.solids(),
        Vec3::new(8.0, 0.0, -2.0),
        Vec3::new(8.0, 0.0, 3.0),
        RADIUS,
    );
    assert!(!inside.start_solid, "{inside:?}");
}

/// Everything a replay would compare, as bits.
fn run(seed: u64) -> Vec<u32> {
    let scene = tunnel_scene();
    let mut rng = Rng::new(seed);
    let mut pos = Vec3::new(9.0, 0.0, 2.0);
    let mut bits = Vec::new();
    for step in 0..3000 {
        if step % 40 == 0 {
            let dir = Vec3::new(
                rng.range_f32(-1.0, 1.0),
                rng.range_f32(-1.0, 1.0),
                rng.range_f32(-0.5, 0.5),
            );
            let vel = dir * 12.0;
            let r = slide_move(scene.solids(), pos, vel, STEP_DT * 40.0, RADIUS);
            pos = r.pos;
            bits.extend([r.vel.x, r.vel.y, r.vel.z].map(f32::to_bits));
        }
        let r = slide_move(
            scene.solids(),
            pos,
            Vec3::new(8.0, 0.0, -0.5),
            STEP_DT,
            RADIUS,
        );
        pos = r.pos;
        bits.extend([pos.x, pos.y, pos.z].map(f32::to_bits));
    }
    bits
}

#[test]
fn a_replay_is_bit_exact() {
    let (a, b) = (run(5), run(5));
    assert_eq!(a, b, "two identical runs, each building the level afresh");
    assert_ne!(a, run(6), "a different walk differs, so the test can tell");
    let xs: Vec<f32> = a.iter().step_by(3).map(|&b| f32::from_bits(b)).collect();
    assert!(xs.iter().any(|x| *x > 12.5), "the walk used the tunnel");
}

fn trace_bits(solids: Solids) -> Vec<u32> {
    let mut rng = Rng::new(77);
    let mut bits = Vec::new();
    for _ in 0..300 {
        let p = |rng: &mut Rng| {
            Vec3::new(
                rng.range_f32(-11.0, 11.0),
                rng.range_f32(-5.0, 5.0),
                rng.range_f32(0.6, 5.0),
            )
        };
        let (a, b) = (p(&mut rng), p(&mut rng));
        let t = sweep_sphere(solids, a, b, RADIUS);
        bits.extend([t.fraction, t.end.x, t.end.y, t.end.z].map(f32::to_bits));
    }
    bits
}

#[test]
fn an_empty_world_changes_nothing() {
    let scene = tunnel_scene();
    let empty = VoxelWorld::default();
    let with = Solids::from(&scene.bsp).with_voxels(&empty);
    assert_eq!(trace_bits(with), trace_bits(Solids::from(&scene.bsp)));
    assert!(!with.is_solid(v(0.0, 0.0, 2.0)));
}

fn vertex(x: f32, y: f32, z: f32) -> VoxelVertex {
    VoxelVertex {
        pos: Vec3::new(x, y, z),
        normal: Vec3::new(0.0, 0.0, 1.0),
        cell: [0; 3],
    }
}

fn hostile_mesh(vertices: Vec<VoxelVertex>, triangles: Vec<[u16; 3]>) -> Arc<VoxelChunk> {
    let bounds = vertices
        .iter()
        .fold(Aabb::empty(), |b, v| b.including(v.pos));
    let mesh = ChunkMesh {
        materials: vec![0; triangles.len()],
        vertices,
        triangles,
        bounds,
        air: AirMask::NONE,
    };
    Arc::new(VoxelChunk {
        index: CellIndex::build(&mesh),
        lods: [mesh.clone(), mesh.clone(), mesh],
        leaves: Vec::new(),
    })
}

#[test]
fn hostile_worlds_do_not_panic() {
    let scene = tunnel_scene();
    let nan = f32::NAN;
    let chunks = vec![
        hostile_mesh(Vec::new(), Vec::new()),
        hostile_mesh(vec![vertex(0.0, 0.0, 0.0)], vec![[0, 1, 2], [u16::MAX; 3]]),
        hostile_mesh(
            vec![
                vertex(nan, 0.0, 0.0),
                vertex(f32::INFINITY, 1.0, 1.0),
                vertex(1.0e30, 0.0, 0.0),
            ],
            vec![[0, 1, 2]],
        ),
        hostile_mesh(
            vec![
                vertex(0.0, 0.0, 0.0),
                vertex(0.0, 0.0, 0.0),
                vertex(0.0, 0.0, 0.0),
            ],
            vec![[0, 1, 2]],
        ),
        hostile_mesh(
            vec![
                vertex(0.0, 0.0, 2.0),
                vertex(4.0, 0.0, 2.0),
                vertex(0.0, 4.0, 2.0),
            ],
            vec![[0, 1, 2]],
        ),
    ];
    let geometry = |size: f32, table: Vec<u32>| VolumeGeometry {
        chunks: [5, 1, 1],
        air: Vec::new(),
        table,
        origin: Vec3::ZERO,
        chunk_size: size,
        rock: vec![true; 2],
    };
    let table = vec![0, 1, 2, 3, 4];
    let worlds = [
        geometry(4.0, table.clone()),
        geometry(0.0, table.clone()),
        geometry(nan, table.clone()),
        geometry(f32::INFINITY, table.clone()),
        geometry(4.0, vec![99, NO_CHUNK, 4]),
    ];
    for g in worlds {
        let world = VoxelWorld {
            chunks: chunks.clone(),
            volumes: vec![g],
        };
        let solids = Solids::from(&scene.bsp).with_voxels(&world);
        let ends = [
            Vec3::new(5.0, 3.0, 2.0),
            Vec3::new(nan, 0.0, 0.0),
            Vec3::new(1.0e30, 0.0, 2.0),
        ];
        for to in ends {
            for radius in [0.0, RADIUS, 3.0, f32::NAN, -1.0] {
                let t = sweep_sphere(solids, Vec3::new(1.0, 1.0, 2.0), to, radius);
                assert!(t.fraction.is_finite() || !to.is_finite() || !radius.is_finite());
                slide_move(solids, Vec3::new(1.0, 1.0, 2.0), to, 0.1, radius);
            }
        }
        solids.is_solid(v(1.0, 1.0, 2.0));
    }
}

#[test]
fn a_flat_triangle_is_a_one_sided_floor() {
    // One triangle in the plane z = 2, winding counter-clockwise from above.
    let chunk = hostile_mesh(
        vec![
            vertex(-4.0, -4.0, 2.0),
            vertex(8.0, -4.0, 2.0),
            vertex(-4.0, 8.0, 2.0),
        ],
        vec![[0, 1, 2]],
    );
    let world = VoxelWorld {
        chunks: vec![chunk],
        volumes: vec![VolumeGeometry {
            chunks: [1, 1, 1],
            air: vec![AirMask::NONE],
            table: vec![0],
            origin: Vec3::new(-4.0, -4.0, -4.0),
            chunk_size: 16.0,
            rock: vec![false],
        }],
    };
    let scene = tunnel_scene();
    let solids = Solids::from(&scene.bsp).with_voxels(&world);
    let from_above = sweep_sphere(
        solids,
        Vec3::new(0.0, 0.0, 4.0),
        Vec3::new(0.0, 0.0, 0.0),
        RADIUS,
    );
    assert!(
        from_above.hit && (from_above.end.z - (2.0 + RADIUS)).abs() < 0.01,
        "{from_above:?}"
    );
    assert!((from_above.normal.z - 1.0).abs() < 1.0e-6);
    // Behind the triangle is rock, which a one-sided triangle lets a sphere leave.
    let from_below = sweep_sphere(
        solids,
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, 4.0),
        RADIUS,
    );
    assert!(!from_below.start_solid && !from_below.hit, "{from_below:?}");
    // Past its edge the plane's extension is not a wall.
    let beside = sweep_sphere(
        solids,
        Vec3::new(10.0, 0.0, 3.0),
        Vec3::new(10.0, 0.0, 1.2),
        RADIUS,
    );
    assert!(!beside.hit, "{beside:?}");
    // Just off the triangle's long edge the sphere rides on the rounded edge
    // instead of falling past it, and is turned away from it.
    let at_edge = sweep_sphere(
        solids,
        Vec3::new(2.3, 2.3, 4.0),
        Vec3::new(2.3, 2.3, 1.0),
        RADIUS,
    );
    assert!(at_edge.hit && at_edge.end.z > 2.2, "{at_edge:?}");
    assert!(
        at_edge.normal.z < 0.95 && at_edge.normal.x > 0.1,
        "{at_edge:?}"
    );
}

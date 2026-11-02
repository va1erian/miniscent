//! What voxel volumes cost, away from the renderer (M11.1, M11.6): meshing,
//! level of detail, connectivity, dabs, the brushes' part and collision. The
//! scene is denser than most real levels: every chunk of a warped gyroid cave
//! holds surface.
//!
//! `cargo run --release -p mscent-bench --example voxel_bench`

use std::hint::black_box;
use std::time::Instant;

use mscent_collide::{STEP_DT, Solids, slide_move, sweep_sphere};
use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_world::brush::{Brush, Op};
use mscent_world::format;
use mscent_world::voxel::{
    CellIndex, CellSize, ChunkMesh, Context, DabOp, Lod, Mesher, Prim, Volume, VoxelWorld,
    apply_dab, fill,
};
use mscent_world::{Level, compile};

const CHUNKS: [u16; 3] = [8, 8, 4];
const REPEATS: usize = 20;
/// Bytes a vertex and a triangle will take in the renderer's form.
const VERTEX_BYTES: usize = 24;
const TRIANGLE_BYTES: usize = 6;

fn cave(p: Vec3d) -> f64 {
    let g = |x: f64, y: f64, z: f64| x.sin() * y.cos() + y.sin() * z.cos() + z.sin() * x.cos();
    let edge =
        p.x.min(32.0 - p.x)
            .min(p.y.min(32.0 - p.y))
            .min(p.z.min(16.0 - p.z));
    let warped = g(p.x * 0.55, p.y * 0.55, p.z * 0.55)
        + g(p.x * 1.7 + 3.0, p.y * 1.7, p.z * 1.7 + 1.0) * 0.18
        + g(p.x * 4.1, p.y * 4.3 + 2.0, p.z * 3.9) * 0.05
        + 0.25;
    warped.max(1.0 - edge)
}

fn coords() -> Vec<[u16; 3]> {
    let mut out = Vec::new();
    for z in 0..CHUNKS[2] {
        for y in 0..CHUNKS[1] {
            for x in 0..CHUNKS[0] {
                out.push([x, y, z]);
            }
        }
    }
    out
}

fn triangles(meshes: &[ChunkMesh]) -> usize {
    meshes.iter().map(|m| m.triangles.len()).sum()
}

fn memory_mib(meshes: &[ChunkMesh]) -> f64 {
    let bytes: usize = meshes
        .iter()
        .map(|m| m.vertices.len() * VERTEX_BYTES + m.triangles.len() * TRIANGLE_BYTES)
        .sum();
    bytes as f64 / f64::from(1 << 20)
}

/// Meshes every chunk, returning the meshes with mean and worst microseconds.
fn mesh_all(volume: &Volume, brushes: &[Brush]) -> (Vec<ChunkMesh>, f64, f64) {
    let mut ctx = Context::new(volume, brushes);
    let mut mesher = Mesher::new();
    let (mut total, mut worst) = (0.0f64, 0.0f64);
    let mut meshes = Vec::new();
    for c in coords() {
        let t = Instant::now();
        meshes.extend(mesher.mesh(volume, &mut ctx, c));
        let us = t.elapsed().as_secs_f64() * 1e6;
        total += us;
        worst = worst.max(us);
    }
    let mean = total / meshes.len().max(1) as f64;
    (meshes, mean, worst)
}

fn report_meshing(volume: &Volume) -> Vec<ChunkMesh> {
    let mut best = mesh_all(volume, &[]);
    for _ in 1..REPEATS {
        let run = mesh_all(volume, &[]);
        if run.1 < best.1 {
            best = run;
        }
    }
    let (meshes, mean, worst) = best;
    println!(
        "mesh per chunk: {mean:.0} us mean, {worst:.0} us worst; {} triangles, {} vertices, {:.1} MiB",
        triangles(&meshes),
        meshes.iter().map(|m| m.vertices.len()).sum::<usize>(),
        memory_mib(&meshes),
    );
    meshes
}

fn report_lod(meshes: &[ChunkMesh]) {
    for (name, lod) in [("LOD1", Lod::Half), ("LOD2", Lod::Quarter)] {
        let t = Instant::now();
        let reduced: Vec<ChunkMesh> = meshes.iter().map(|m| m.clustered(lod)).collect();
        let us = t.elapsed().as_secs_f64() * 1e6 / meshes.len().max(1) as f64;
        println!(
            "{name}: {} triangles of {}, {us:.0} us per chunk",
            triangles(&reduced),
            triangles(meshes)
        );
    }
}

/// Median microseconds of a dab of radius `r`, each on a fresh copy so copy on
/// write is paid every time, as a dab shortly after a preview clone pays it.
fn dab_us(volume: &Volume, brushes: &[Brush], r: f64, op: DabOp) -> f64 {
    let Some(prim) = Prim::sphere(Vec3d::new(16.0, 16.0, 8.0), r) else {
        return 0.0;
    };
    let mut ctx = Context::new(volume, brushes);
    let mut runs: Vec<f64> = (0..REPEATS)
        .map(|_| {
            let mut copy = volume.clone();
            let t = Instant::now();
            black_box(apply_dab(&mut copy, &mut ctx, &prim, op));
            t.elapsed().as_secs_f64() * 1e6
        })
        .collect();
    runs.sort_by(f64::total_cmp);
    runs[runs.len() / 2]
}

fn report_dabs(volume: &Volume) {
    for r in [0.5, 1.0, 2.0, 4.0, 8.0] {
        println!(
            "dab r={r}: carve {:.0} us, smooth {:.0} us",
            dab_us(volume, &[], r, DabOp::Carve),
            dab_us(volume, &[], r, DabOp::Smooth),
        );
    }
}

/// The price of `base` and the ring: an untouched subtract volume under a room
/// of plain boxes, meshed from a cold context.
fn report_brushes() {
    let v = |x, y, z| Vec3d::new(x, y, z);
    let cuboid = |lo, hi, op, order| Brush::cuboid(lo, hi, op, order);
    let brushes: Result<Vec<Brush>, _> = (0..8u32)
        .map(|i| {
            let f = f64::from(i) * 4.0;
            cuboid(v(f, f, 2.0), v(f + 6.0, f + 6.0, 14.0), Op::Subtract, i)
        })
        .collect();
    let volume = Volume::new(Op::Subtract, 100, CellSize::Quarter, [0; 3], CHUNKS);
    let (Ok(brushes), Ok(volume)) = (brushes, volume) else {
        println!("brush scene refused");
        return;
    };
    let (meshes, mean, worst) = mesh_all(&volume, &brushes);
    println!(
        "subtract volume over 8 brushes, cold: {mean:.0} us mean, {worst:.0} us worst, {} triangles",
        triangles(&meshes)
    );
}

/// What the volume adds to a level file, and how long it takes to read back.
fn report_file(volume: &Volume) {
    let level = Level {
        volumes: vec![volume.clone()],
        ..Default::default()
    };
    let bytes = format::write(&level);
    let t = Instant::now();
    let read = format::read(&bytes).is_ok();
    println!(
        "level file: {:.2} MiB, read back in {:.1} ms ({read})",
        bytes.len() as f64 / f64::from(1 << 20),
        t.elapsed().as_secs_f64() * 1e3
    );
}

/// Positions in the cave's air, found by asking the world, with a seed so every
/// run measures the same sweeps.
fn air_positions(solids: Solids, n: usize) -> Vec<Vec3> {
    let mut rng = Rng::new(9);
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let p = Vec3::new(
            rng.range_f32(1.0, 31.0),
            rng.range_f32(1.0, 31.0),
            rng.range_f32(1.0, 15.0),
        );
        if !sweep_sphere(solids, p, p, 0.5).start_solid && !solids.is_solid(p.to_f64()) {
            out.push(p);
        }
    }
    out
}

fn random_unit(rng: &mut Rng) -> Vec3 {
    loop {
        let v = Vec3::new(
            rng.range_f32(-1.0, 1.0),
            rng.range_f32(-1.0, 1.0),
            rng.range_f32(-1.0, 1.0),
        );
        if let Some(n) = v.normalized() {
            return n;
        }
    }
}

/// Mean nanoseconds of `f` over `starts`, best of a few passes.
fn per_call_ns<T>(starts: &[Vec3], mut f: impl FnMut(Vec3, Vec3) -> T) -> f64 {
    let mut rng = Rng::new(3);
    let dirs: Vec<Vec3> = starts.iter().map(|_| random_unit(&mut rng)).collect();
    let mut best = f64::MAX;
    for _ in 0..REPEATS {
        let t = Instant::now();
        for (&p, &d) in starts.iter().zip(&dirs) {
            black_box(f(p, d));
        }
        best = best.min(t.elapsed().as_secs_f64() * 1e9 / starts.len() as f64);
    }
    best
}

/// The cave as a level, so it has a BSP, and what sweeping it costs (M11.6).
fn report_collision(volume: &Volume) {
    let level = Level {
        volumes: vec![volume.clone()],
        ..Default::default()
    };
    let Ok(bsp) = compile(&level.csg_brushes()) else {
        println!("collision scene refused");
        return;
    };
    let t = Instant::now();
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    println!(
        "VoxelWorld::build: {:.0} ms, {} chunks",
        t.elapsed().as_secs_f64() * 1e3,
        world.chunks.len()
    );
    let t = Instant::now();
    let indexes: Vec<CellIndex> = (world.chunks.iter())
        .map(|c| CellIndex::build(&c.lods[0]))
        .collect();
    let bytes: usize = indexes.iter().map(CellIndex::bytes).sum();
    println!(
        "cell index: {:.0} us a chunk, {:.2} MiB for {} triangles",
        t.elapsed().as_secs_f64() * 1e6 / indexes.len().max(1) as f64,
        bytes as f64 / f64::from(1 << 20),
        (world.chunks.iter())
            .map(|c| c.lods[0].triangles.len())
            .sum::<usize>(),
    );
    let solids = Solids::from(&bsp).with_voxels(&world);
    let starts = air_positions(solids, 2000);
    for (name, radius) in [("radius 0.5", 0.5), ("radius 0 (a ray)", 0.0)] {
        for (what, length) in [("one step, 0.1", 0.1), ("a long move, 4", 4.0)] {
            let ns = per_call_ns(&starts, |p, d| {
                sweep_sphere(solids, p, p + d * length, radius)
            });
            println!("sweep {name}, {what}: {:.2} us", ns / 1e3);
        }
    }
    let ns = per_call_ns(&starts, |p, d| {
        slide_move(solids, p, d * 10.0, STEP_DT, 0.5)
    });
    println!(
        "slide_move, 10 units/s for one step, radius 0.5: {:.2} us",
        ns / 1e3
    );
    let bare = Solids::from(&bsp);
    let ns = per_call_ns(&starts, |p, d| sweep_sphere(bare, p, p + d * 0.1, 0.5));
    println!(
        "sweep one step, BSP only, same positions: {:.2} us",
        ns / 1e3
    );
    let ns = per_call_ns(&starts, |p, _| solids.is_solid(p.to_f64()));
    println!("is_solid, BSP and voxels: {:.2} us", ns / 1e3);
}

fn main() {
    // A subtract volume with no brushes: rock on all sides, so the only surface
    // is the cave's, not a hull around the box as well.
    let Ok(mut volume) = Volume::new(Op::Subtract, 0, CellSize::Quarter, [0; 3], CHUNKS) else {
        println!("volume refused");
        return;
    };
    let mut ctx = Context::new(&volume, &[]);
    let t = Instant::now();
    fill(&mut volume, &mut ctx, cave, 0);
    println!(
        "{CHUNKS:?} chunks of 16^3 at 0.25: filled in {:.1} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
    report_file(&volume);
    let meshes = report_meshing(&volume);
    report_lod(&meshes);
    report_dabs(&volume);
    report_brushes();
    report_collision(&volume);
}

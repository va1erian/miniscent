//! Bake time of a level with a big voxel cave in it, with and without voxel
//! occluders, and what the records cost.
//!
//! `cargo run --release -p mscent-render --example voxel_bake [chunks_xy chunks_z]`
//!
//! The scene is a room around a warped gyroid of rock in a volume of
//! `chunks_xy x chunks_xy x chunks_z` chunks of 0.25 cells (8 8 4 by default: 32
//! by 32 by 16 units, every chunk mixed, about 220 thousand triangles). It is lit
//! by four lamps found in its air, with the game's bake settings. Each row is the
//! whole bake on every core unless it says otherwise.

use std::sync::atomic::AtomicBool;
use std::time::Instant;

use mscent_math::{Vec3, Vec3d};
use mscent_render::BuildConfig;
use mscent_world::brush::{Brush, Op};
use mscent_world::lightmap::{
    BakeConfig, CellLighting, CellSites, LightFace, Lightmap, Occluders, PointLight, bake_in,
    light_cells,
};
use mscent_world::materials::{FLOOR, NO_DRAW};
use mscent_world::voxel::{CellSize, Context, Volume, VoxelWorld, fill};
use mscent_world::{Entity, Level, compile, extract_faces};

type Failure = Box<dyn std::error::Error>;

fn gyroid(p: Vec3d) -> f64 {
    let g = |x: f64, y: f64, z: f64| x.sin() * y.cos() + y.sin() * z.cos() + z.sin() * x.cos();
    // Warped, so no cell is flat, as in the design's benchmark scene.
    let w = Vec3d::new(
        p.x + 0.6 * (p.y * 0.3).sin(),
        p.y + 0.6 * (p.z * 0.3).sin(),
        p.z + 0.6 * (p.x * 0.3).sin(),
    );
    g(w.x * 0.55, w.y * 0.55, w.z * 0.55) + 0.25
}

/// Points of the cave's air, spread out, for lamps.
fn lamp_spots(lo: Vec3d, size: Vec3d) -> Vec<Vec3> {
    let mut found = Vec::new();
    for k in 0..4 {
        let t = (f64::from(k) + 0.5) / 4.0;
        let guess = lo + Vec3d::new(size.x * t, size.y * (1.0 - t), size.z * 0.5);
        // The nearest air point on a small walk from the guess.
        let air = (0..2000).find_map(|i| {
            let d = f64::from(i) * 0.05;
            let p = guess + Vec3d::new(d.cos() * d * 0.2, d.sin() * d * 0.2, 0.0);
            (gyroid(p) < -0.3).then_some(p)
        });
        found.extend(air.map(Vec3d::to_f32));
    }
    found
}

fn level(xy: u16, z: u16) -> Result<Level, Failure> {
    let side = f64::from(xy) * 4.0;
    let height = f64::from(z) * 4.0;
    let room = Brush::cuboid(
        Vec3d::new(-2.0, -2.0, -2.0),
        Vec3d::new(side + 2.0, side + 2.0, height + 2.0),
        Op::Subtract,
        0,
    )
    .map_err(|e| format!("{e:?}"))?
    .with_material(FLOOR);
    let mut volume = Volume::new(Op::Add, 1, CellSize::Quarter, [0; 3], [xy, xy, z])
        .map_err(|e| format!("{e:?}"))?;
    let shape = volume.clone();
    fill(&mut volume, &mut Context::new(&shape, &[]), gyroid, 4);
    let spots = lamp_spots(Vec3d::ZERO, Vec3d::new(side, side, height));
    let entities = spots
        .into_iter()
        .map(|pos| Entity::Light {
            pos,
            color: Vec3::new(1.0, 0.95, 0.85),
            radius: 40.0,
            intensity: 1.0,
        })
        .collect();
    Ok(Level {
        brushes: vec![room],
        volumes: vec![volume],
        entities,
        ..Default::default()
    })
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn main() -> Result<(), Failure> {
    let args: Vec<u16> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let (xy, z) = (
        args.first().copied().unwrap_or(8),
        args.get(1).copied().unwrap_or(4),
    );
    let level = level(xy, z)?;
    let brushes = level.csg_brushes();
    let bsp = compile(&brushes).map_err(|e| format!("{e:?}"))?;
    let mut faces = extract_faces(&bsp, &brushes);
    faces.retain(|f| f.material != NO_DRAW);
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    let light_faces: Vec<LightFace> = faces.iter().map(LightFace::from).collect();
    let build = BuildConfig::default();
    let lights: Vec<PointLight> = level
        .entities
        .iter()
        .filter_map(|e| match *e {
            Entity::Light {
                pos,
                color,
                radius,
                intensity,
            } => Some(PointLight {
                position: pos,
                color: color * (intensity * build.light_gain),
                radius,
                cone: None,
            }),
            _ => None,
        })
        .collect();
    let cfg = BakeConfig {
        luxel_size: build.luxel_size,
        ambient: build.ambient,
        bounce_gain: 0.08,
        light_radius: build.light_radius,
        shadow_samples: build.shadow_samples,
        ..BakeConfig::default()
    };
    let triangles: usize = world.chunks.iter().map(|c| c.lods[0].triangles.len()).sum();
    let t = Instant::now();
    let sites = CellSites::of(&world);
    println!(
        "{} chunks, {triangles} triangles, {} faces, {} lamps, {} cells (sites in {:.0} ms, records {:.2} MiB)",
        world.chunks.len(),
        light_faces.len(),
        lights.len(),
        sites.len(),
        ms(t),
        (sites.len() * 9) as f64 / (1 << 20) as f64,
    );
    let t = Instant::now();
    let both = Occluders::new(&bsp, &world);
    println!("occluders built in {:.0} ms", ms(t));
    let brushes_only = Occluders::brushes(&bsp);
    let stop = AtomicBool::new(false);
    let none = CellSites::default();
    let run = |label: &str, occ: &Occluders, sites: &CellSites| -> Result<Lightmap, Failure> {
        let t = Instant::now();
        let map = bake_in(occ, sites, &light_faces, &lights, &cfg, &stop)
            .map_err(|e| format!("{e:?}"))?;
        println!("{label:<52} {:>9.0} ms", ms(t));
        Ok(map)
    };
    println!("-- whole bake, {} threads", cfg.workers);
    run("faces only (no voxel cells)", &brushes_only, &none)?;
    run("faces only, shadowed by voxels as well", &both, &none)?;
    run(
        "faces and cells, brush occluders only",
        &brushes_only,
        &sites,
    )?;
    run("faces and cells, voxel occluders", &both, &sites)?;
    println!("-- the cells' direct light alone, one thread, no bounce");
    let all: Vec<usize> = (0..sites.len()).collect();
    for (label, occ) in [
        ("brush occluders only", &brushes_only),
        ("voxel occluders", &both),
    ] {
        let one = BakeConfig { workers: 1, ..cfg };
        let env = CellLighting {
            occ,
            lights: &lights,
            cfg: &one,
        };
        let t = Instant::now();
        let records = light_cells(&env, &sites, &all);
        let took = ms(t);
        println!(
            "{label:<52} {took:>9.0} ms  ({:.1} us a cell, {} records)",
            took * 1e3 / records.len().max(1) as f64,
            records.len()
        );
    }
    Ok(())
}

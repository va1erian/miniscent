//! What scale saves in the lightmap: the same apparent mountain range, built at
//! scale 1 and as a miniature at scale 16, with the bake each costs.
//!
//!   cargo run --release -p mscent-render --example scale_lightmap
//!
//! The range is 256 units across and 64 tall as it looks. At scale 1 that is a
//! 256-unit volume of one-unit cells in a 288-unit room; at scale 16 it is 16
//! units across in cells of 1/8, in an 18-unit room, seen with parallax 1/16.

use std::path::Path;
use std::time::Instant;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{BuildConfig, LevelRenderer, MaterialTable};
use mscent_world::materials::{METAL, ROCK, VIEW_FIRST};
use mscent_world::vista_scale::{luxel_factor, parallax_of_scale};
use mscent_world::voxel::{CellSize, MountainRange, mountains};
use mscent_world::{Brush, Entity, Level, MAX_VISTA_FOG, Op};

const AWAY: f64 = 1000.0;

fn vd(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn cuboid(min: Vec3d, max: Vec3d, op: Op, order: u32, material: u16) -> Brush {
    Brush::cuboid(min, max, op, order)
        .expect("a cuboid")
        .with_material(material)
}

/// The scene shrunk by `shrink`: 1 for as it looks, 16 for the miniature.
fn scene(shrink: f64) -> Level {
    let s = |v: f64| v / shrink;
    let cell = if shrink == 1.0 {
        CellSize::One
    } else {
        CellSize::Eighth
    };
    let play = cuboid(
        vd(-100.0, -16.0, -16.0),
        vd(100.0, 48.0, 16.0),
        Op::Subtract,
        0,
        VIEW_FIRST,
    );
    let (lo, hi) = (
        vd(AWAY - s(144.0), s(-144.0), s(-8.0)),
        vd(AWAY + s(144.0), s(144.0), s(120.0)),
    );
    let range = MountainRange {
        min: vd(AWAY - s(128.0), s(-128.0), s(-6.0)),
        max: vd(
            AWAY + s(128.0),
            s(128.0),
            s(-6.0) + 2.0 * cell.units() + s(64.0) + 1.0,
        ),
        cell,
        peak: s(64.0),
        seed: 11,
        material: 4,
        order: 4,
        clear: None,
        edge: None,
    };
    let pillar = |x: f64, order: u32| {
        let (a, b) = (
            vd(AWAY + s(x), s(-20.0), s(-8.0)),
            vd(AWAY + s(x + 12.0), s(-8.0), s(40.0)),
        );
        cuboid(a, b, Op::Add, order, METAL)
    };
    let floor_top = vd(AWAY + s(144.0), s(144.0), s(-5.0));
    let brushes = vec![
        play,
        cuboid(lo, hi, Op::Subtract, 1, METAL),
        cuboid(lo, floor_top, Op::Add, 2, ROCK),
        pillar(-60.0, 6),
        pillar(40.0, 7),
    ];
    let layer = Entity::VistaLayer {
        pos: Vec3::new(AWAY as f32, 0.0, s(20.0) as f32),
        centre: Vec3::ZERO,
        slot: 0,
        layer: 0,
        parallax: parallax_of_scale(shrink as f32),
        spin: Vec3::ZERO,
        pan: [0.0, 0.0],
        fog_start: 0.0,
        fog_end: MAX_VISTA_FOG,
        fog_color: 0,
    };
    let lamp = Entity::Light {
        pos: Vec3::new((AWAY + s(-100.0)) as f32, s(-60.0) as f32, s(90.0) as f32),
        color: Vec3::new(1.0, 0.95, 0.85),
        radius: s(400.0) as f32,
        intensity: 1.0,
    };
    Level {
        brushes,
        volumes: vec![mountains(&range).expect("a range")],
        entities: vec![layer, lamp],
        ..Level::default()
    }
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    let cfg = BuildConfig {
        shadow_samples: 16,
        ..BuildConfig::default()
    };
    println!("scale  factor  faces  atlas luxels  cell records  chunks  bake ms");
    for shrink in [1.0, 16.0] {
        let level = scene(shrink);
        let chunks: usize = level
            .volumes
            .iter()
            .map(|v| {
                v.chunks()
                    .iter()
                    .map(|&c| usize::from(c))
                    .product::<usize>()
            })
            .sum();
        let started = Instant::now();
        let r = LevelRenderer::from_level(&level, materials.clone(), &cfg).expect("builds");
        let ms = started.elapsed().as_secs_f64() * 1e3;
        let map = r.lightmap();
        let luxels: u64 = map
            .projections()
            .iter()
            .map(|p| u64::from(p.width) * u64::from(p.height))
            .sum();
        println!(
            "{shrink:>5}  {:>6}  {:>5}  {luxels:>12}  {:>12}  {chunks:>6}  {ms:>7.0}",
            luxel_factor(parallax_of_scale(shrink as f32)),
            map.projections().len(),
            map.cells().len(),
        );
    }
}

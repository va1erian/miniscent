//! Pictures of voxel volumes beside real brush geometry, through the level
//! renderer the game uses. Fuchsia sky behind everything shows any hole in a
//! surface, and each picture reports how many such pixels it has, the triangles
//! it drew and the time it took.
//!
//! `cargo run --release -p mscent-render --example voxel_shot -- target/shots/voxel`

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_render::png_encode::encode_png;
use mscent_render::{
    BuildConfig, Compiled, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::brush::{Brush, Op};
use mscent_world::materials::{BRICK, FLOOR, METAL, ROCK};
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume, apply_dab, fill};
use mscent_world::{Entity, Level, format};

const SIZE: (u32, u32) = (1280, 720);
const HOLE: u32 = 0xffff_00ff;
const ROCK_ID: u8 = ROCK as u8;
const FLOOR_ID: u8 = FLOOR as u8;
const METAL_ID: u8 = METAL as u8;
const BRICK_ID: u8 = BRICK as u8;

type Failure = Box<dyn std::error::Error>;

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn cuboid(lo: Vec3d, hi: Vec3d, op: Op, order: u32, material: u16) -> Result<Brush, Failure> {
    let b = Brush::cuboid(lo, hi, op, order).map_err(|e| format!("{e:?}"))?;
    Ok(b.with_material(material))
}

fn dab(volume: &mut Volume, ctx: &mut Context, prim: Option<Prim>, op: DabOp) {
    if let Some(p) = prim {
        apply_dab(volume, ctx, &p, op);
    }
}

fn lamp(x: f32, y: f32, z: f32, radius: f32) -> Entity {
    Entity::Light {
        pos: Vec3::new(x, y, z),
        color: Vec3::new(1.0, 0.95, 0.85),
        radius,
        intensity: 1.0,
    }
}

fn renderer(level: &Level) -> Result<LevelRenderer, Failure> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let table = MaterialTable::load_dir(&dir).map_err(|e| format!("{e:?}"))?;
    let cfg = BuildConfig {
        luxel_size: 0.5,
        sky_top: HOLE,
        sky_bottom: HOLE,
        fog_start: 200.0,
        fog_end: 400.0,
        ..BuildConfig::default()
    };
    let c = Compiled::new(level, &cfg).map_err(|e| format!("{e:?}"))?;
    let map = Arc::new(c.draft().map_err(|e| format!("{e:?}"))?);
    c.into_renderer(map, table)
        .map_err(|e| format!("{e:?}").into())
}

/// Writes the view from `eye` towards `target`.
fn shoot(r: &LevelRenderer, eye: Vec3, target: Vec3, out: &Path) -> Result<(), Failure> {
    let (w, h) = SIZE;
    let to = target - eye;
    let yaw = (-to.x).atan2(to.y);
    let pitch = to.z.atan2(to.x.hypot(to.y));
    let view = View::from_angles(eye, yaw, pitch, 0.0);
    let mut fb = Framebuffer::new(w, h).ok_or("bad size")?;
    let mut stats = r
        .render(&mut fb, &view, Quality::High, &[], None)
        .ok_or("bad view")?;
    let t = Instant::now();
    for _ in 0..20 {
        stats = r
            .render(&mut fb, &view, Quality::High, &[], None)
            .ok_or("bad view")?;
    }
    let ms = t.elapsed().as_secs_f64() * 1e3 / 20.0;
    let holes = fb.pixels().iter().filter(|&&p| p == HOLE).count();
    let png = encode_png(w, h, fb.pixels()).ok_or("cannot encode")?;
    std::fs::write(out, png)?;
    println!(
        "{}: {} voxel triangles, {holes} hole pixels, {ms:.1} ms",
        out.display(),
        stats.voxel_triangles,
    );
    Ok(())
}

/// A cave dug into a brush wall: everything left of the volume's box is the
/// plain brush wall, and the rock inside the box still follows it.
fn cave_entrance(dir: &Path) -> Result<(), Failure> {
    let room = cuboid(
        v(-20.0, -20.0, 0.0),
        v(20.0, 0.0, 10.0),
        Op::Subtract,
        0,
        ROCK,
    )?;
    let brushes = vec![room];
    let mut volume = Volume::new(Op::Subtract, 1, CellSize::Quarter, [-32, -8, -8], [4, 4, 3])
        .map_err(|e| format!("{e:?}"))?;
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &brushes);
    let mut rng = Rng::new(21);
    // The tunnel: a wandering chain of round cones going in through the wall.
    let (mut at, mut radius) = (v(0.0, -1.5, 2.3), 2.3);
    for _ in 0..9 {
        let next = at + v(rng.range_f64(-1.0, 1.0), 1.5, rng.range_f64(-0.3, 0.45));
        let r = (radius + rng.range_f64(-0.3, 0.25)).clamp(1.3, 2.4);
        dab(
            &mut volume,
            &mut ctx,
            Prim::round_cone(at, next, radius, r),
            DabOp::Carve,
        );
        (at, radius) = (next, r);
    }
    // Weathering: lumps and bites around the mouth, then a smoothing pass.
    for _ in 0..70 {
        let p = v(
            rng.range_f64(-5.0, 5.0),
            rng.range_f64(-0.3, 5.0),
            rng.range_f64(0.0, 7.0),
        );
        let prim = Prim::sphere(p, rng.range_f64(0.25, 0.7));
        let op = if rng.below(2) == 0 {
            DabOp::Carve
        } else {
            DabOp::Add { material: ROCK_ID }
        };
        dab(&mut volume, &mut ctx, prim, op);
    }
    for _ in 0..40 {
        let p = v(
            rng.range_f64(-5.0, 5.0),
            rng.range_f64(-0.5, 7.0),
            rng.range_f64(0.0, 7.0),
        );
        dab(&mut volume, &mut ctx, Prim::sphere(p, 1.6), DabOp::Smooth);
    }
    let level = Level {
        brushes,
        volumes: vec![volume],
        entities: vec![lamp(-4.0, -8.0, 7.0, 34.0), lamp(0.5, 5.5, 2.4, 9.0)],
        ..Default::default()
    };
    let r = renderer(&level)?;
    let target = Vec3::new(0.0, 3.0, 2.3);
    shoot(
        &r,
        Vec3::new(-7.5, -11.0, 3.5),
        target,
        &dir.join("cave-entrance.png"),
    )?;
    shoot(
        &r,
        Vec3::new(0.6, -1.0, 2.2),
        Vec3::new(0.5, 5.0, 2.4),
        &dir.join("cave-inside.png"),
    )
}

fn stalagmite(volume: &mut Volume, ctx: &mut Context, rng: &mut Rng, x: f64, y: f64, up: bool) {
    let (root, sign) = if up { (-0.4, 1.0) } else { (10.4, -1.0) };
    let height = rng.range_f64(1.4, 3.8);
    let base = rng.range_f64(0.4, 0.75);
    let mid = v(
        x + rng.range_f64(-0.15, 0.15),
        y + rng.range_f64(-0.15, 0.15),
        root + sign * height * 0.55,
    );
    let tip = v(
        x + rng.range_f64(-0.3, 0.3),
        y + rng.range_f64(-0.3, 0.3),
        root + sign * height,
    );
    let m = DabOp::Add { material: ROCK_ID };
    dab(
        volume,
        ctx,
        Prim::round_cone(v(x, y, root), mid, base, base * 0.5),
        m,
    );
    dab(volume, ctx, Prim::round_cone(mid, tip, base * 0.5, 0.2), m);
}

fn mushroom(volume: &mut Volume, ctx: &mut Context, at: Vec3d, size: f64) {
    let top = at + v(0.15 * size, 0.0, 1.9 * size);
    let cap = Prim::sphere(top, 1.15 * size);
    dab(volume, ctx, cap, DabOp::Add { material: BRICK_ID });
    // Cutting the sphere's underside leaves a cap; the stem then goes in.
    let under = Prim::sphere(top - v(0.0, 0.0, 0.75 * size), 1.05 * size);
    dab(volume, ctx, under, DabOp::Carve);
    let stem = Prim::round_cone(at, top, 0.42 * size, 0.24 * size);
    dab(volume, ctx, stem, DabOp::Add { material: FLOOR_ID });
    dab(volume, ctx, Prim::sphere(at, 0.6 * size), DabOp::Smooth);
}

fn coral(
    volume: &mut Volume,
    ctx: &mut Context,
    rng: &mut Rng,
    at: Vec3d,
    dir: Vec3d,
    r: f64,
    depth: u32,
) {
    let next = at + dir * (r * rng.range_f64(2.6, 3.8));
    let end = (r * 0.78).max(0.17);
    dab(
        volume,
        ctx,
        Prim::round_cone(at, next, r, end),
        DabOp::Add { material: METAL_ID },
    );
    if depth == 0 {
        return;
    }
    for _ in 0..2 {
        let turn = v(
            rng.range_f64(-0.8, 0.8),
            rng.range_f64(-0.8, 0.8),
            rng.range_f64(0.1, 0.7),
        );
        let bent = (dir + turn).normalized().unwrap_or(dir);
        coral(volume, ctx, rng, next, bent, end, depth - 1);
    }
}

/// A brush room with things grown in it, in an add volume.
fn garden(dir: &Path) -> Result<(), Failure> {
    let floor = cuboid(
        v(-12.0, -12.0, 0.0),
        v(12.0, 12.0, 10.0),
        Op::Subtract,
        0,
        METAL,
    )?;
    let brushes = vec![floor.clone()];
    let mut volume = Volume::new(Op::Add, 1, CellSize::Quarter, [-40, -40, -4], [5, 5, 3])
        .map_err(|e| format!("{e:?}"))?;
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &brushes);
    let mut rng = Rng::new(4);
    for _ in 0..9 {
        let (x, y) = (rng.range_f64(-9.0, 9.0), rng.range_f64(0.5, 9.0));
        stalagmite(&mut volume, &mut ctx, &mut rng, x, y, true);
    }
    for _ in 0..13 {
        let (x, y) = (rng.range_f64(-9.0, 9.0), rng.range_f64(-3.0, 9.0));
        stalagmite(&mut volume, &mut ctx, &mut rng, x, y, false);
    }
    for (x, y, s) in [
        (-3.4, -3.2, 1.0),
        (-1.6, -4.4, 0.65),
        (-4.8, -1.4, 0.75),
        (-5.3, -4.5, 0.5),
    ] {
        mushroom(&mut volume, &mut ctx, v(x, y, -0.1), s);
    }
    for (x, y) in [(3.4, -3.0), (5.6, -0.6), (1.6, 0.6)] {
        coral(
            &mut volume,
            &mut ctx,
            &mut rng,
            v(x, y, -0.2),
            v(0.0, 0.0, 1.0),
            0.5,
            4,
        );
    }
    let level = Level {
        brushes,
        volumes: vec![volume],
        entities: vec![lamp(0.0, -4.0, 8.0, 30.0)],
        ..Default::default()
    };
    shoot(
        &renderer(&level)?,
        Vec3::new(-0.5, -10.5, 2.9),
        Vec3::new(0.0, 0.0, 1.9),
        &dir.join("garden.png"),
    )
}

/// A warped gyroid, positive solid.
fn alien_field(p: Vec3d) -> f64 {
    let g = |x: f64, y: f64, z: f64| x.sin() * y.cos() + y.sin() * z.cos() + z.sin() * x.cos();
    g(p.x * 0.55, p.y * 0.55, p.z * 0.55)
        + g(p.x * 1.7 + 3.0, p.y * 1.7, p.z * 1.7 + 1.0) * 0.18
        + g(p.x * 4.1, p.y * 4.3 + 2.0, p.z * 3.9) * 0.05
        + 0.25
}

/// How far a line of sight runs from `from` along `dir` before it meets rock.
fn clear_run(from: Vec3d, dir: Vec3d) -> f64 {
    (1..120)
        .map(|i| f64::from(i) * 0.25)
        .find(|&t| alien_field(from + dir * t) >= 0.0)
        .unwrap_or(30.0)
}

/// A seeded look around for the place in the cave with the longest view: an
/// eye in rock would see the backs of the surfaces, and through them.
fn best_view() -> (Vec3, Vec3) {
    let mut rng = Rng::new(8);
    let mut best = (0.0, Vec3d::ZERO, Vec3d::ZERO);
    for _ in 0..4000 {
        let eye = v(
            rng.range_f64(8.0, 24.0),
            rng.range_f64(8.0, 24.0),
            rng.range_f64(5.0, 11.0),
        );
        if alien_field(eye) > -0.3 {
            continue;
        }
        let turn = v(
            rng.range_f64(-1.0, 1.0),
            rng.range_f64(-1.0, 1.0),
            rng.range_f64(-0.3, 0.3),
        );
        let Some(dir) = turn.normalized() else {
            continue;
        };
        let run = clear_run(eye, dir);
        if run > best.0 {
            best = (run, eye, dir);
        }
    }
    (best.1.to_f32(), (best.1 + best.2 * 4.0).to_f32())
}

/// Inside a gyroid cave with no brushes near: no face draws at all.
fn alien_cave(dir: &Path) -> Result<(), Failure> {
    let brushes: Vec<Brush> = Vec::new();
    let mut volume = Volume::new(Op::Subtract, 1, CellSize::Quarter, [0; 3], [8, 8, 4])
        .map_err(|e| format!("{e:?}"))?;
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &brushes);
    fill(&mut volume, &mut ctx, alien_field, METAL_ID);
    let (eye, target) = best_view();
    let level = Level {
        volumes: vec![volume],
        entities: vec![lamp(eye.x, eye.y, eye.z + 0.5, 26.0)],
        ..Default::default()
    };
    shoot(&renderer(&level)?, eye, target, &dir.join("alien-cave.png"))
}

/// Where each saved showcase level is looked at from: eye, then target. The
/// levels are written by the editor's generators (`MSCENT_BLESS=1 cargo test -p
/// mscent-editor showcase`), one per use case, and these are the poses its
/// budget test uses.
const SHOWCASES: [(&str, [f32; 3], [f32; 3]); 5] = [
    ("hive", [0.0, 3.0, 5.0], [0.0, 12.0, 5.0]),
    ("coral", [0.0, -8.0, 3.5], [0.0, 3.0, 1.8]),
    ("mushrooms", [0.0, -8.0, 2.5], [0.0, 3.0, 1.5]),
    ("tree", [0.0, -11.0, 4.0], [0.0, 4.0, 6.0]),
    ("cave", [0.0, -3.0, 2.6], [0.0, 8.0, 2.6]),
];

/// The levels the editor's generators built, drawn from their saved files.
fn showcases(dir: &Path) -> Result<(), Failure> {
    let levels = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/levels");
    for (name, eye, target) in SHOWCASES {
        let bytes = std::fs::read(levels.join(format!("voxel-{name}.msl")))?;
        let level = format::read(&bytes).map_err(|e| format!("{name}: {e:?}"))?;
        let r = renderer(&level)?;
        let png = dir.join(format!("{name}.png"));
        shoot(
            &r,
            Vec3::new(eye[0], eye[1], eye[2]),
            Vec3::new(target[0], target[1], target[2]),
            &png,
        )?;
    }
    Ok(())
}

fn main() -> Result<(), Failure> {
    let dir = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("target/shots/voxel"), PathBuf::from);
    std::fs::create_dir_all(&dir)?;
    cave_entrance(&dir)?;
    garden(&dir)?;
    alien_cave(&dir)?;
    showcases(&dir)
}

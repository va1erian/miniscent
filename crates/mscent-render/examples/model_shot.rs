//! What enemy models cost in a real frame (M15.2), and a picture of them. Each
//! design is timed through the level's own pass, on one worker, against the same
//! view with no model, and through the flat path that draws today's enemy.
//!
//! `cargo run --release -p mscent-render --example model_shot -- <textures dir> target/shots/models.png`
//!
//! The textures directory defaults to `assets/textures`, which does not yet hold
//! plating, chitin or glass: those draw grey there.

use std::path::{Path, PathBuf};
use std::time::Instant;

use mscent_math::{Quat, Vec3};
use mscent_render::camera::Camera;
use mscent_render::mesh::{Mesh, MeshLighting, MeshVertex, Pose, Triangle, draw_mesh};
use mscent_render::model::{ModelId, ModelInstance, ModelTable};
use mscent_render::png_encode::encode_png;
use mscent_render::{
    BuildConfig, Effects, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::Entity;
use mscent_world::Level;
use mscent_world::brush::{Brush, Op};
use mscent_world::materials::METAL;
use mscent_world::model::{Model, PartMesh, build, read};

type Failure = Box<dyn std::error::Error>;

const FRAME: (u32, u32) = (640, 360);
const REPEATS: usize = 40;
const STOCK: [(&str, &str); 3] = [
    ("sentry", include_str!("../../../assets/enemies/sentry.msb")),
    ("dart", include_str!("../../../assets/enemies/dart.msb")),
    ("mantis", include_str!("../../../assets/enemies/mantis.msb")),
];
/// A flat colour per role, so the flat path shows its parts apart.
const FLAT: u32 = 0xff90_9090;

fn room() -> Result<Level, Failure> {
    let lo = mscent_math::Vec3d::new(-16.0, -16.0, -8.0);
    let hi = mscent_math::Vec3d::new(16.0, 16.0, 8.0);
    let box_room = Brush::cuboid(lo, hi, Op::Subtract, 0)
        .map_err(|e| format!("{e:?}"))?
        .with_material(METAL);
    let lamp = |x: f32, y: f32, z: f32, c: Vec3, intensity: f32| Entity::Light {
        pos: Vec3::new(x, y, z),
        color: c,
        radius: 40.0,
        intensity,
    };
    Ok(Level {
        brushes: vec![box_room],
        entities: vec![
            lamp(-6.0, -8.0, 6.0, Vec3::new(1.0, 0.95, 0.85), 1.0),
            lamp(8.0, 6.0, 2.0, Vec3::new(0.5, 0.6, 1.0), 0.6),
        ],
        ..Default::default()
    })
}

fn flat_mesh(model: &Model, level: usize) -> Option<Mesh> {
    let mut verts = Vec::new();
    let mut tris = Vec::new();
    for part in &model.parts {
        let mesh: &PartMesh = &part.lods[level];
        let base = u16::try_from(verts.len()).ok()?;
        verts.extend(mesh.vertices.iter().map(|v| MeshVertex {
            pos: v.pos,
            normal: v.normal,
        }));
        for t in &mesh.triangles {
            tris.push(Triangle {
                indices: t.map(|i| i + base),
                color: FLAT,
            });
        }
    }
    Mesh::new(verts, tris)
}

fn best_ms(mut f: impl FnMut()) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..REPEATS {
        let t = Instant::now();
        f();
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
    }
    best
}

fn render_ms(
    r: &LevelRenderer,
    fb: &mut Framebuffer,
    view: &View,
    quality: Quality,
    models: Option<(&ModelTable, &[ModelInstance])>,
) -> (f64, u32) {
    let effects = Effects {
        models,
        ..Effects::default()
    };
    let mut triangles = 0;
    let ms = best_ms(|| {
        let stats = r.render_with_effects(fb, view, quality, &[], None, &effects);
        triangles = stats.map_or(0, |s| s.model_triangles);
    });
    (ms, triangles)
}

fn main() -> Result<(), Failure> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures"),
        PathBuf::from,
    );
    let out = args.next().map(PathBuf::from);
    let materials = || MaterialTable::load_dir(&dir).map_err(|e| format!("{e:?}"));
    let cfg = BuildConfig {
        shadow_samples: 8,
        luxel_size: 0.5,
        ..BuildConfig::default()
    };
    let mut r =
        LevelRenderer::from_level(&room()?, materials()?, &cfg).map_err(|e| format!("{e:?}"))?;
    r.set_tiling(FRAME.1, 1);
    let mut table = ModelTable::new();
    let mut made = Vec::new();
    for (name, text) in STOCK {
        let blueprint = read(text.as_bytes())?;
        let model = build(&blueprint)?;
        let id = table.add(&blueprint, &model, &materials()?);
        made.push((name, id, model, blueprint.scale as f32));
    }
    let eye = Vec3::new(-7.0, -5.0, 2.0);
    let view = View::from_angles(eye, -(7.0f32).atan2(5.0) + core::f32::consts::PI, -0.2, 0.0);
    let ahead = view.forward();
    let target = eye + ahead * 8.0;
    let mut fb = Framebuffer::new(FRAME.0, FRAME.1).ok_or("frame")?;
    println!("one worker, {}x{}, best of {REPEATS}", FRAME.0, FRAME.1);
    for quality in [Quality::Low, Quality::High] {
        let (base, _) = render_ms(&r, &mut fb, &view, quality, None);
        println!("{quality:?}: room alone {base:.2} ms");
        for (name, id, _, _) in &made {
            let one = [ModelInstance::new(*id, target, Quat::IDENTITY)];
            let (ms, tris) = render_ms(&r, &mut fb, &view, quality, Some((&table, &one)));
            println!("  {name:<8} {tris:>5} tris  adds {:>6.3} ms", ms - base);
        }
        let crowd: Vec<ModelInstance> = (0..12)
            .map(|i| {
                let (a, b) = ((i % 4) as f32 - 1.5, (i / 4) as f32);
                let across = Vec3::new(ahead.y, -ahead.x, 0.0);
                let at = target + across * (a * 2.5) + ahead * (b * 3.0);
                ModelInstance::new(made[i % made.len()].1, at, Quat::IDENTITY)
            })
            .collect();
        let (ms, tris) = render_ms(&r, &mut fb, &view, quality, Some((&table, &crowd)));
        println!("  12 mixed {tris:>5} tris  adds {:>6.3} ms", ms - base);
    }
    flat_row(&made, target, &view)?;
    if let Some(out) = out {
        write_picture(&table, &made, &out, &dir)?;
    }
    Ok(())
}

/// The path today's enemy takes, on the same meshes and from the same view.
fn flat_row(
    made: &[(&str, ModelId, Model, f32)],
    target: Vec3,
    view: &View,
) -> Result<(), Failure> {
    let cam = view
        .camera(1.1, 0.1, 400.0, FRAME.0, FRAME.1)
        .ok_or("camera")?;
    let cam: Camera = cam;
    let light = MeshLighting::default();
    let mut fb = Framebuffer::new(FRAME.0, FRAME.1).ok_or("frame")?;
    println!("flat path, same meshes, on an empty frame:");
    for (name, _, model, scale) in made {
        let mesh = flat_mesh(model, 0).ok_or("mesh")?;
        let pose = Pose {
            pos: target,
            yaw: 0.0,
            scale: *scale,
        };
        let ms = best_ms(|| {
            fb.clear(0xff00_0000);
            draw_mesh(&mut fb, &cam, &mesh, &pose, &light, None);
        });
        println!(
            "  {name:<8} {:>5} tris  {ms:>6.3} ms",
            mesh.triangles().len()
        );
    }
    Ok(())
}

/// The designs side by side, large enough to judge, each with the same light.
fn write_picture(
    table: &ModelTable,
    made: &[(&str, ModelId, Model, f32)],
    out: &Path,
    dir: &Path,
) -> Result<(), Failure> {
    let materials = MaterialTable::load_dir(dir).map_err(|e| format!("{e:?}"))?;
    let cfg = BuildConfig {
        shadow_samples: 8,
        luxel_size: 0.5,
        ..BuildConfig::default()
    };
    let r = LevelRenderer::from_level(&room()?, materials, &cfg).map_err(|e| format!("{e:?}"))?;
    let eye = Vec3::new(0.0, -13.0, 2.5);
    let view = View::from_angles(eye, 0.0, -0.1, 0.0);
    let turn = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), 0.6);
    let instances: Vec<ModelInstance> = made
        .iter()
        .zip([-6.5f32, 0.0, 6.5])
        .map(|((_, id, _, _), x)| ModelInstance::new(*id, Vec3::new(x, 0.0, 1.0), turn))
        .collect();
    let mut fb = Framebuffer::new(1280, 720).ok_or("frame")?;
    let effects = Effects {
        models: Some((table, &instances)),
        ..Effects::default()
    };
    r.render_with_effects(&mut fb, &view, Quality::High, &[], None, &effects)
        .ok_or("view")?;
    let png = encode_png(1280, 720, fb.pixels()).ok_or("cannot encode")?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out, png)?;
    Ok(())
}

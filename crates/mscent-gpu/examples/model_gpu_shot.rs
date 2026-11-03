//! What enemy models cost on the GPU: the showcase's hall with twelve of the stock
//! designs down it, against the same frame with none, and against software:
//! `cargo run --release -p mscent-gpu --example model_gpu_shot -- out.png 1920 1080`
//!
//! The stock skins are not in `assets/textures` yet, so they draw grey. Software's
//! picture of the same frame goes beside it, as `out.png.software.png`.

use std::path::Path;
use std::time::Instant;

use mscent_gpu::{Finish, GpuRenderer};
use mscent_math::{Quat, Vec3};
use mscent_render::model::{ModelInstance, ModelTable};
use mscent_render::png_encode::encode_png;
use mscent_render::{
    BuildConfig, Effects, FrameInput, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::model::{build, read};
use mscent_world::showcase_level;

const FRAMES: usize = 60;
const STOCK: [&str; 3] = [
    include_str!("../../../assets/enemies/sentry.msb"),
    include_str!("../../../assets/enemies/dart.msb"),
    include_str!("../../../assets/enemies/mantis.msb"),
];

/// Median of `costs`, in milliseconds.
fn median(mut costs: Vec<f64>) -> f64 {
    costs.sort_by(f64::total_cmp);
    costs[costs.len() / 2]
}

fn time(mut frame: impl FnMut() -> Result<(), String>) -> Result<f64, String> {
    let mut costs = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        let start = Instant::now();
        frame()?;
        costs.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(median(costs))
}

/// The stock designs, four of each, spread down the hall seen from `view`.
fn crowd(
    materials: &MaterialTable,
    view: &View,
) -> Result<(ModelTable, Vec<ModelInstance>), String> {
    let mut table = ModelTable::new();
    let mut ids = Vec::new();
    for text in STOCK {
        let blueprint = read(text.as_bytes()).map_err(|e| format!("{e:?}"))?;
        let model = build(&blueprint).map_err(|e| format!("{e:?}"))?;
        ids.push(table.add(&blueprint, &model, materials));
    }
    let forward = view.forward();
    let across = Vec3::new(forward.y, -forward.x, 0.0);
    let instances = (0..12)
        .map(|i| {
            let ahead = 5.0 + 2.5 * (i / 3) as f32;
            let side = (i % 3) as f32 - 1.0;
            let pos = view.pos + forward * ahead + across * (side * 2.0);
            let turn = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), i as f32 * 0.7);
            ModelInstance::new(ids[i % ids.len()], pos, turn)
        })
        .collect();
    Ok((table, instances))
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args
        .first()
        .ok_or("usage: model_gpu_shot <out.png> [width height]")?;
    let side = |i: usize, default: u32| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(default);
    let (width, height) = (side(1, 1920), side(2, 1080));
    let view = View::from_angles(Vec3::new(3.0, 3.0, 2.5), -0.78, 0.0, 0.0);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let load = || MaterialTable::load_dir(&dir).map_err(|e| format!("{e:?}"));
    let (table, instances) = crowd(&load()?, &view)?;
    let software = LevelRenderer::from_level(&showcase_level(), load()?, &BuildConfig::default())
        .map_err(|e| format!("{e:?}"))?;
    let gpu = mscent_gpu::Gpu::new().map_err(|e| e.to_string())?;
    let finish = Finish::of(Quality::High, &gpu);
    println!("adapter: {}", gpu.adapter());
    let assets = software.assets();
    let mut renderer =
        GpuRenderer::with_finish(gpu, assets, finish).map_err(|why| why.to_string())?;
    let mut fb = Framebuffer::new(width, height).ok_or("bad size")?;
    for (name, models) in [
        ("none", None),
        ("12 models", Some((&table, &instances[..]))),
    ] {
        let effects = Effects {
            models,
            ..Effects::default()
        };
        let frame = FrameInput {
            view: &view,
            quality: Quality::High,
            actors: &[],
            effects,
            wants_depth: false,
        };
        let mut triangles = 0;
        let gpu_ms = time(|| {
            let stats = renderer.draw(assets, &mut fb, &frame).ok_or("no frame")?;
            triangles = stats.model_triangles;
            Ok(())
        })?;
        if let Some(error) = renderer.take_error() {
            return Err(error);
        }
        let mut soft = Framebuffer::new(width, height).ok_or("bad size")?;
        let soft_ms = time(|| {
            let drawn =
                software.render_with_effects(&mut soft, &view, Quality::High, &[], None, &effects);
            drawn.map(|_| ()).ok_or_else(|| "no frame".to_string())
        })?;
        println!(
            "{width}x{height} {name}: GPU {finish:?} {gpu_ms:.2} ms, software {soft_ms:.2} ms, {triangles} model triangles"
        );
    }
    let png = encode_png(width, height, fb.pixels()).ok_or("cannot encode")?;
    std::fs::write(out, png).map_err(|e| e.to_string())?;
    let effects = Effects {
        models: Some((&table, &instances[..])),
        ..Effects::default()
    };
    software
        .render_with_effects(&mut fb, &view, Quality::High, &[], None, &effects)
        .ok_or("no frame")?;
    let png = encode_png(width, height, fb.pixels()).ok_or("cannot encode")?;
    std::fs::write(format!("{out}.software.png"), png).map_err(|e| e.to_string())
}

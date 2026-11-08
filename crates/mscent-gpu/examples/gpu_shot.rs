//! Draws a level on the GPU and writes a PNG, with no window, timing the draw
//! against software's at the same size:
//! `cargo run --release -p mscent-gpu --example gpu_shot -- out.png 3840 2160`
//! draws the showcase with the finish High gets, `reference` after the size draws
//! software's, `plain` after that (or after the size) switches the GPU-only looks
//! (parallax, shadows) off, and
//! `... -- out.png 3840 2160 assets/levels/voxel-cave.msl 8 8 4 0.5 -0.2`
//! a level file from an eye at x y z, turned by yaw and pitch.

use std::path::Path;
use std::time::Instant;

use mscent_gpu::{Finish, GpuEffects, GpuRenderer};
use mscent_math::Vec3;
use mscent_render::png_encode::encode_png;
use mscent_render::{
    BuildConfig, Effects, FrameInput, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::{Level, showcase_level};

const FRAMES: usize = 60;

/// Median and worst of `costs`, in milliseconds.
fn summary(mut costs: Vec<f64>) -> (f64, f64) {
    costs.sort_by(f64::total_cmp);
    (costs[costs.len() / 2], costs[costs.len() - 1])
}

/// The level and view the arguments name, or the showcase's hall.
fn scene(args: &[String]) -> Result<(Level, View), String> {
    let Some(path) = args.first() else {
        let view = View::from_angles(Vec3::new(3.0, 3.0, 2.5), -0.78, 0.0, 0.0);
        return Ok((showcase_level(), view));
    };
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let level = mscent_world::format::read(&bytes).map_err(|e| format!("{path}: {e:?}"))?;
    let number = |i: usize| -> Result<f32, String> {
        let text = args.get(i).ok_or("a level needs x y z yaw pitch")?;
        text.parse().map_err(|_| format!("not a number: {text}"))
    };
    let eye = Vec3::new(number(1)?, number(2)?, number(3)?);
    Ok((level, View::from_angles(eye, number(4)?, number(5)?, 0.0)))
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args
        .first()
        .ok_or("usage: gpu_shot <out.png> [width height [reference] [level x y z yaw pitch]]")?;
    let side = |i: usize, default: u32| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(default);
    let (width, height) = (side(1, 1280), side(2, 720));
    let word = |i: usize, w: &str| args.get(i).is_some_and(|a| a == w);
    let reference = word(3, "reference");
    let plain = word(3 + usize::from(reference), "plain");
    let rest = 3 + usize::from(reference) + usize::from(plain);
    let (level, view) = scene(args.get(rest..).unwrap_or(&[]))?;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).map_err(|e| format!("{e:?}"))?;
    let software = LevelRenderer::from_level(&level, materials, &BuildConfig::default())
        .map_err(|e| format!("{e:?}"))?;
    let gpu = mscent_gpu::Gpu::new().map_err(|e| e.to_string())?;
    let finish = if reference {
        Finish::Reference
    } else {
        Finish::of(Quality::High, &gpu)
    };
    println!("adapter: {}", gpu.adapter());
    let assets = software.assets();
    let start = Instant::now();
    let mut renderer =
        GpuRenderer::with_finish(gpu, assets, finish).map_err(|why| why.to_string())?;
    renderer.set_effects(plain.then_some(GpuEffects::OFF));
    println!("upload: {:.2} ms", start.elapsed().as_secs_f64() * 1000.0);
    let mut fb = Framebuffer::new(width, height).ok_or("bad size")?;
    let frame = FrameInput {
        view: &view,
        quality: Quality::High,
        actors: assets.actors(),
        effects: Effects {
            models: assets.spawn_models(),
            ..Effects::default()
        },
        wants_depth: false,
    };
    let (mut costs, mut triangles) = (Vec::with_capacity(FRAMES), 0);
    for _ in 0..FRAMES {
        let start = Instant::now();
        triangles = renderer
            .draw(assets, &mut fb, &frame)
            .ok_or("no frame")?
            .voxel_triangles;
        costs.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    if let Some(error) = renderer.take_error() {
        return Err(error);
    }
    let (median, worst) = summary(costs);
    println!(
        "{width}x{height} GPU {finish:?}: median {median:.2} ms, worst {worst:.2} ms, {triangles} voxel triangles"
    );
    let mut reference = Framebuffer::new(width, height).ok_or("bad size")?;
    let costs = (0..FRAMES / 4)
        .map(|_| {
            let start = Instant::now();
            software.render(&mut reference, &view, Quality::High, assets.actors(), None);
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    let (median, worst) = summary(costs);
    println!("{width}x{height} software: median {median:.2} ms, worst {worst:.2} ms");
    let png = encode_png(width, height, fb.pixels()).ok_or("cannot encode")?;
    std::fs::write(out, png).map_err(|e| e.to_string())
}

//! Draws the showcase on the GPU and writes a PNG, with no window:
//! `cargo run --release -p mscent-gpu --example gpu_shot -- out.png 3840 2160`

use std::path::Path;
use std::time::Instant;

use mscent_math::Vec3;
use mscent_render::png_encode::encode_png;
use mscent_render::{
    BuildConfig, Effects, FrameInput, FrameRenderer, Framebuffer, LevelRenderer, MaterialTable,
    Quality, View,
};
use mscent_world::showcase_level;

const FRAMES: usize = 60;

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let out = args
        .next()
        .ok_or("usage: gpu_shot <out.png> [width height]")?;
    let mut side = |default: u32| args.next().and_then(|s| s.parse().ok()).unwrap_or(default);
    let (width, height) = (side(1280), side(720));
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).map_err(|e| format!("{e:?}"))?;
    let software = LevelRenderer::from_level(&showcase_level(), materials, &BuildConfig::default())
        .map_err(|e| format!("{e:?}"))?;
    let gpu = mscent_gpu::Gpu::new().map_err(|e| e.to_string())?;
    println!("adapter: {}", gpu.adapter());
    let mut renderer = mscent_gpu::GpuRenderer::new(gpu, software.into_assets())
        .map_err(|(why, _)| why.to_string())?;
    let mut fb = Framebuffer::new(width, height).ok_or("bad size")?;
    let view = View::from_angles(Vec3::new(3.0, 3.0, 2.5), -0.78, 0.0, 0.0);
    let frame = FrameInput {
        view: &view,
        quality: Quality::High,
        actors: &[],
        effects: Effects::default(),
    };
    let mut costs = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        let start = Instant::now();
        renderer.draw(&mut fb, &frame).ok_or("no frame")?;
        costs.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    if let Some(error) = renderer.take_error() {
        return Err(error);
    }
    costs.sort_by(f64::total_cmp);
    let (median, worst) = (costs[FRAMES / 2], costs[FRAMES - 1]);
    println!("{width}x{height}: median {median:.2} ms, worst {worst:.2} ms");
    let png = encode_png(width, height, fb.pixels()).ok_or("cannot encode")?;
    std::fs::write(&out, png).map_err(|e| e.to_string())
}

//! Headless frame-cost benchmark on a fixed reference scene (M3.8).
//!
//! The scene is the level generated from seed 1 with default parameters,
//! viewed from the first room at eight yaw angles, rendered at 640x360 through
//! the tile renderer. The figure CI gates is the ratio of the frame cost to a
//! calibration loop timed in the same process: both slow down together on a
//! slow or busy machine, so the ratio is far more stable than milliseconds.
//! The absolute 16.6 ms budget is judged on the reference box instead.
//!
//!   cargo run --release -p mscent-render --example frame_bench -- [--workers N]
//!       [--frames N] [--batches N] [--json]
//!
//! Each of `batches` rounds times a batch of calibration passes and a batch of
//! frames; the reported cost is the fastest batch of each, which discards
//! interference from other processes.

use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use mscent_math::Vec3;
use mscent_render::scene::Scene;
use mscent_render::simd::{Level, scale_rgb};
use mscent_render::{Camera, Fog, Framebuffer, MaterialTable, TileRenderer};
use mscent_world::{GenParams, Portals, compile, extract_faces, generate, room_centers};

const W: u32 = 640;
const H: u32 = 360;
const TILE_ROWS: u32 = 24;
const SEED: u64 = 1;
const VIEWS: usize = 8;

struct Options {
    workers: usize,
    frames: usize,
    batches: usize,
    json: bool,
}

fn parse_options() -> Result<Options, String> {
    let mut o = Options {
        workers: 1,
        frames: 48,
        batches: 7,
        json: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut num = |name: &str| -> Result<usize, String> {
            args.next()
                .and_then(|v| v.parse().ok())
                .filter(|&n| n > 0)
                .ok_or(format!("{name} needs a positive number"))
        };
        match a.as_str() {
            "--workers" => o.workers = num("--workers")?,
            "--frames" => o.frames = num("--frames")?,
            "--batches" => o.batches = num("--batches")?,
            "--json" => o.json = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(o)
}

fn build_scene() -> Result<(Scene, Vec3), String> {
    let params = GenParams::default();
    let brushes = generate(SEED, &params);
    let bsp = compile(&brushes).map_err(|e| format!("compile: {e:?}"))?;
    let faces = extract_faces(&bsp, &brushes);
    let portals = Portals::build(&bsp);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    // Real textures keep the cache behaviour honest; the grey fallback would
    // make every texel fetch hit the same line.
    let materials = MaterialTable::load_dir(&dir).unwrap_or_default();
    let eye = room_centers(SEED, &params)
        .first()
        .ok_or("generator made no rooms")?
        .to_f32();
    Ok((Scene::new(bsp, faces, portals, materials), eye))
}

fn cameras(eye: Vec3) -> Result<Vec<Camera>, String> {
    (0..VIEWS)
        .map(|i| {
            let yaw = i as f32 * std::f32::consts::TAU / VIEWS as f32 + 0.3;
            let target = eye + Vec3::new(yaw.cos(), yaw.sin(), 0.0) * 10.0;
            Camera::look_at(eye, target, Vec3::new(0.0, 0.0, 1.0), 1.2, 0.1, 400.0, W, H)
                .ok_or_else(|| "bad camera".to_string())
        })
        .collect()
}

/// Fixed workload standing in for "one machine-speed unit": a full-screen
/// textured, lit fill with a lookup table that exceeds L1, the same mix of
/// integer work and memory traffic the rasterizer has.
fn calibration_pass(table: &[u32], out: &mut [u32], pass: u32) -> u32 {
    for (i, px) in out.iter_mut().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        let u = x
            .wrapping_mul(41)
            .wrapping_add(y.wrapping_mul(13))
            .wrapping_add(pass * 7);
        let v = x
            .wrapping_mul(9)
            .wrapping_add(y.wrapping_mul(37))
            .wrapping_add(pass * 3);
        *px = table[((v >> 3 & 127) * 128 + (u >> 3 & 127)) as usize];
    }
    scale_rgb(out, 200 + pass % 50);
    out.iter().fold(0, |s, &p| s.wrapping_add(p))
}

fn time_ms(frames: usize, mut run: impl FnMut(usize)) -> f64 {
    let t = Instant::now();
    for f in 0..frames {
        run(f);
    }
    t.elapsed().as_secs_f64() * 1000.0 / frames as f64
}

fn main() {
    let opts = parse_options().unwrap_or_else(|e| {
        eprintln!("frame_bench: {e}");
        std::process::exit(2);
    });
    if let Err(e) = run(&opts) {
        eprintln!("frame_bench: {e}");
        std::process::exit(1);
    }
}

fn run(opts: &Options) -> Result<(), String> {
    let (scene, eye) = build_scene()?;
    let cams = cameras(eye)?;
    let fog = Fog::new(0xff20_2830, 30.0, 200.0);
    let mut renderer =
        TileRenderer::new(W, H, TILE_ROWS, opts.workers).ok_or("bad render target")?;
    let mut fb = Framebuffer::new(W, H).ok_or("bad framebuffer")?;
    let table: Vec<u32> = (0..128 * 128u32)
        .map(|i| i.wrapping_mul(2654435761))
        .collect();
    let mut calib_out = vec![0u32; (W * H) as usize];

    let frame = |f: usize, fb: &mut Framebuffer, r: &mut TileRenderer| {
        let cam = &cams[f % VIEWS];
        let draw = |band: &mut Framebuffer| {
            band.clear_sky(0xff30_4860, 0xff80_8890);
            scene.render(band, cam, fog);
        };
        r.render(fb, &draw).map_err(|_| "size mismatch".to_string())
    };
    // Warm caches, thread machinery and lazy initialisation.
    for f in 0..VIEWS {
        frame(f, &mut fb, &mut renderer)?;
    }
    let (mut best_frame, mut best_calib) = (f64::MAX, f64::MAX);
    for _ in 0..opts.batches {
        best_calib = best_calib.min(time_ms(opts.frames, |p| {
            black_box(calibration_pass(&table, &mut calib_out, p as u32));
        }));
        let mut failed = None;
        best_frame = best_frame.min(time_ms(opts.frames, |f| {
            if let Err(e) = frame(f, &mut fb, &mut renderer) {
                failed = Some(e);
            }
            black_box(&fb);
        }));
        if let Some(e) = failed {
            return Err(e);
        }
    }
    let pixels = fb.pixels().iter().fold(0u32, |s, &p| s.wrapping_add(p));
    report(opts, best_frame, best_calib, pixels);
    Ok(())
}

fn report(opts: &Options, frame_ms: f64, calib_ms: f64, checksum: u32) {
    let ratio = frame_ms / calib_ms;
    if opts.json {
        println!(
            "{{\"ms_per_frame\": {frame_ms:.4}, \"calibration_ms\": {calib_ms:.4}, \"ratio\": {ratio:.4}, \"workers\": {}}}",
            opts.workers
        );
        return;
    }
    println!(
        "reference scene: generated level seed {SEED}, {W}x{H}, {VIEWS} views, tiles of {TILE_ROWS} rows"
    );
    println!("simd level      {:?}", Level::best());
    println!("workers         {}", opts.workers);
    println!("frame           {frame_ms:8.3} ms");
    println!("calibration     {calib_ms:8.3} ms");
    println!("ratio           {ratio:8.4}  (frame / calibration, the CI figure)");
    println!("last frame checksum {checksum:08x}");
}

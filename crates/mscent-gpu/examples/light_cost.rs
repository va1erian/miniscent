//! What each dynamic light costs a frame on the GPU (M17.1 of
//! `docs/dynamic-lights.md`; results in `docs/perf/dynamic-lights.md`):
//! `cargo run --release -p mscent-gpu --example light_cost -- [options]`
//!
//! Options (all optional):
//! - `--levels showcase,voxel-cave`
//! - `--sizes 1920x1080,3840x2160`
//! - `--counts 0,4,16,64,256`: lights in the frame
//! - `--radius 6`: the lights' radius in world units
//! - `--runs 5`: repeats of each measurement; the median of their medians is printed
//!
//! At the shipped cap only the first `MAX_DYN_LIGHTS` lights are drawn, and the
//! output says so. The numbers in the doc came from a throwaway build with the cap
//! raised and every light looped for every pixel; see the doc for how to redo it.
//!
//! Per case: the CPU side of a frame, the time until the GPU is done, and the GPU
//! time of a frame when 30 are queued back to back (CPU encoding overlapped, so it
//! is the GPU's own cost). Nothing is read back. Also the cost of placing a light
//! (`Scene::light_reach`) and the size of a cluster grid at each size.

use std::path::Path;
use std::time::Instant;

use mscent_gpu::{Finish, Gpu, GpuRenderer};
use mscent_math::Vec3;
use mscent_render::{
    BuildConfig, DynLight, Effects, FrameInput, LevelAssets, LevelRenderer, MAX_DYN_LIGHTS,
    MaterialTable, Quality, View,
};
use mscent_world::{Level, showcase_level};

const FRAMES: usize = 60;
const QUEUED: usize = 30;

struct Options {
    levels: Vec<String>,
    sizes: Vec<(u32, u32)>,
    counts: Vec<usize>,
    radius: f32,
    runs: usize,
}

fn list<T>(text: &str, parse: impl Fn(&str) -> Option<T>) -> Result<Vec<T>, String> {
    text.split(',')
        .map(|s| parse(s).ok_or(format!("bad value {s:?}")))
        .collect()
}

fn options() -> Result<Options, String> {
    let mut o = Options {
        levels: vec!["showcase".into(), "voxel-cave".into()],
        sizes: vec![(1920, 1080), (3840, 2160)],
        counts: vec![0, 4, 16, 64, 256],
        radius: 6.0,
        runs: 5,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--levels" => o.levels = list(&value, |s| Some(s.to_string()))?,
            "--sizes" => {
                o.sizes = list(&value, |s| {
                    let (w, h) = s.split_once('x')?;
                    Some((w.parse().ok()?, h.parse().ok()?))
                })?
            }
            "--counts" => o.counts = list(&value, |s| s.parse().ok())?,
            "--radius" => o.radius = value.parse().map_err(|_| "bad radius")?,
            "--runs" => o.runs = value.parse().map_err(|_| "bad runs")?,
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    Ok(o)
}

fn level(name: &str) -> Result<Level, String> {
    if name == "showcase" {
        return Ok(showcase_level());
    }
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../assets/levels/{name}.msl"));
    let bytes = std::fs::read(&path).map_err(|e| format!("{name}: {e}"))?;
    mscent_world::format::read(&bytes).map_err(|e| format!("{name}: {e:?}"))
}

fn eye_of(name: &str) -> Vec3 {
    match name {
        "facility" => Vec3::new(-12.0, 0.0, 1.5),
        "voxel-cave" => Vec3::new(8.0, 8.0, 4.0),
        _ => Vec3::new(3.0, 3.0, 2.5),
    }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// A small deterministic generator, so every run places the same lights.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn reaches_anything(light: &DynLight, faces: usize) -> bool {
    (0..faces).any(|f| light.reaches_face(f))
}

/// `count` lights scattered within 10 units of `eye`, each in open space (it
/// reaches at least one face). Also the median time to place one, in microseconds.
fn place(assets: &LevelAssets, eye: Vec3, count: usize, radius: f32) -> (Vec<DynLight>, f64) {
    let scene = assets.scene();
    let faces = scene.faces().len();
    let mut rng = Lcg(7);
    let (mut lights, mut times) = (vec![], vec![]);
    let mut tries = 0;
    while lights.len() < count && tries < 20_000 {
        tries += 1;
        let mut jitter = || (rng.next() - 0.5) * 20.0;
        let pos = eye + Vec3::new(jitter(), jitter(), jitter() * 0.3);
        let mut light = DynLight::new();
        let t = Instant::now();
        light.place(scene, pos, radius, Vec3::new(1.0, 0.8, 0.5));
        let us = t.elapsed().as_secs_f64() * 1e6;
        if reaches_anything(&light, faces) {
            times.push(us);
            lights.push(light);
        }
    }
    let per = if times.is_empty() { 0.0 } else { median(times) };
    (lights, per)
}

struct Timing {
    /// Faces and voxel triangles submitted in the first frame of a run.
    faces: usize,
    triangles: u32,
    cpu: f64,
    done: f64,
    gpu: f64,
}

fn run(
    renderer: &mut GpuRenderer,
    gpu: &Gpu,
    assets: &LevelAssets,
    eye: Vec3,
    size: (u32, u32),
    lights: &[DynLight],
) -> Result<Timing, String> {
    let draw = |renderer: &mut GpuRenderer, i: usize| {
        let yaw = i as f32 / FRAMES as f32 * std::f32::consts::TAU;
        let view = View::from_angles(eye, yaw, -0.1, 0.0);
        let frame = FrameInput {
            view: &view,
            quality: Quality::High,
            actors: assets.actors(),
            effects: Effects {
                lights,
                ..Effects::default()
            },
            wants_depth: false,
        };
        renderer.draw_unread(assets, size, &frame).ok_or("no frame")
    };
    let first = draw(renderer, 0)?;
    gpu.wait();
    let (mut cpu, mut done) = (vec![], vec![]);
    for i in 1..=FRAMES {
        let t = Instant::now();
        draw(renderer, i)?;
        cpu.push(t.elapsed().as_secs_f64() * 1000.0);
        gpu.wait();
        done.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let t = Instant::now();
    for i in 0..QUEUED {
        draw(renderer, i * FRAMES / QUEUED)?;
    }
    gpu.wait();
    let gpu_ms = t.elapsed().as_secs_f64() * 1000.0 / QUEUED as f64;
    if let Some(error) = renderer.take_error() {
        return Err(error);
    }
    Ok(Timing {
        faces: first.drawn.len(),
        triangles: first.voxel_triangles,
        cpu: median(cpu),
        done: median(done),
        gpu: gpu_ms,
    })
}

fn clusters(size: (u32, u32)) {
    let tiles = u64::from(size.0.div_ceil(16)) * u64::from(size.1.div_ceil(16));
    for slices in [16u64, 24] {
        let n = tiles * slices;
        // A count and an offset (2 x u32), then up to 32 u32 indices a cluster.
        let bytes = n * (8 + 32 * 4);
        println!(
            "clusters {}x{}: {tiles} tiles x {slices} slices = {n} clusters, {:.1} MiB of lists at 32 a cluster ({:.2} MiB of headers)",
            size.0,
            size.1,
            bytes as f64 / 1048576.0,
            (n * 8) as f64 / 1048576.0
        );
    }
}

fn measure(name: &str, o: &Options) -> Result<(), String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).map_err(|e| format!("{e:?}"))?;
    let software = LevelRenderer::from_level(&level(name)?, materials, &BuildConfig::default())
        .map_err(|e| format!("{e:?}"))?;
    let assets = software.assets();
    let gpu = Gpu::new().map_err(|e| e.to_string())?;
    let finish = Finish::of(Quality::High, &gpu);
    let mut renderer =
        GpuRenderer::with_finish(gpu.clone(), assets, finish).map_err(|e| e.to_string())?;
    let eye = eye_of(name);
    println!(
        "\n## {name} ({} faces), High {finish:?}, radius {}",
        assets.scene().faces().len(),
        o.radius
    );
    for &size in &o.sizes {
        for &count in &o.counts {
            let (lights, place_us) = place(assets, eye, count, o.radius);
            let drawn = count.min(MAX_DYN_LIGHTS);
            let mut runs = vec![];
            for _ in 0..o.runs {
                runs.push(run(&mut renderer, &gpu, assets, eye, size, &lights)?);
            }
            let m = |f: fn(&Timing) -> f64| median(runs.iter().map(f).collect());
            let first = &runs[0];
            println!(
                "{}x{} lights {count:>3} (drawn {drawn:>3}): CPU {:.3} ms, done {:.3} ms, GPU queued {:.3} ms; place {place_us:.1} us/light; first frame {} faces, {} voxel triangles",
                size.0,
                size.1,
                m(|t| t.cpu),
                m(|t| t.done),
                m(|t| t.gpu),
                first.faces,
                first.triangles,
            );
        }
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let o = options()?;
    println!(
        "MAX_DYN_LIGHTS {MAX_DYN_LIGHTS}, {} runs of {FRAMES} frames, medians of medians",
        o.runs
    );
    for &size in &o.sizes {
        clusters(size);
    }
    for name in &o.levels {
        measure(name, &o)?;
    }
    Ok(())
}

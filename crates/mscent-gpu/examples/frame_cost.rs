//! What a frame costs on the GPU against the plan's budgets
//! (`docs/hardware-renderer.md`, "Costs and budget"), turning a full circle in
//! a level over 240 frames at 3840x2160:
//! `cargo run --release -p mscent-gpu --example frame_cost`, with `plain` after `--`
//! for the same with the GPU-only looks (parallax, shadows) switched off, and
//! `noshadows` for only the shadows off. Two dynamic lights stand near the eye and
//! six enemy models around it, so that the shadows have lights and casters: the
//! cost of shadows is the difference between this and `noshadows`.
//!
//! For each frame: the CPU side, which lists, encodes and submits it with nothing
//! read back, as the direct route does before it presents, and the time until the
//! GPU is done, from the same start. Also how long putting the level on the GPU
//! took, which on the first level of a device includes compiling its pipelines.
//! Allocations are not counted: a counting allocator is `unsafe`, which
//! this crate forbids.

use std::path::Path;
use std::time::Instant;

use mscent_gpu::{Finish, Gpu, GpuEffects, GpuRenderer};
use mscent_math::{Quat, Vec3};
use mscent_render::model::{ModelInstance, ModelTable};
use mscent_render::{
    BuildConfig, DynLight, Effects, FrameInput, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::model::{build, read};
use mscent_world::{Level, showcase_level};

const SENTRY: &str = include_str!("../../../assets/enemies/sentry.msb");

const SIZE: (u32, u32) = (3840, 2160);
const FRAMES: usize = 240;

fn level(name: &str) -> Result<Level, String> {
    if name == "showcase" {
        return Ok(showcase_level());
    }
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../assets/levels/{name}.msl"));
    let bytes = std::fs::read(&path).map_err(|e| format!("{name}: {e}"))?;
    mscent_world::format::read(&bytes).map_err(|e| format!("{name}: {e:?}"))
}

/// Median, 95th percentile and worst of `values`.
fn spread(mut values: Vec<f64>) -> (f64, f64, f64) {
    values.sort_by(f64::total_cmp);
    let at = |p: f64| values[((values.len() - 1) as f64 * p).round() as usize];
    (at(0.5), at(0.95), at(1.0))
}

/// Two lights beside `eye`, and six sentries around it.
fn crowd(
    software: &LevelRenderer,
    materials: &MaterialTable,
    eye: Vec3,
) -> Result<(ModelTable, Vec<ModelInstance>, Vec<DynLight>), String> {
    let blueprint = read(SENTRY.as_bytes()).map_err(|e| format!("{e:?}"))?;
    let model = build(&blueprint).map_err(|e| format!("{e:?}"))?;
    let mut table = ModelTable::new();
    let id = table.add(&blueprint, &model, materials);
    let turn = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), 0.5);
    let enemies = (0..6)
        .map(|i| {
            let around = i as f32 * std::f32::consts::TAU / 6.0;
            let pos = eye + Vec3::new(around.cos() * 4.0, around.sin() * 4.0, 0.0);
            ModelInstance::new(id, pos, turn)
        })
        .collect();
    let mut lights = Vec::new();
    for offset in [Vec3::new(1.5, 0.0, 1.0), Vec3::new(-1.5, 1.0, 0.5)] {
        let mut light = DynLight::new();
        if !light.place(
            software.scene(),
            eye + offset,
            14.0,
            Vec3::new(2.0, 1.6, 1.0),
        ) {
            return Err("a light was refused".into());
        }
        lights.push(light);
    }
    Ok((table, enemies, lights))
}

fn measure(name: &str, eye: Vec3, quality: Quality) -> Result<(), String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let load = || MaterialTable::load_dir(&dir).map_err(|e| format!("{e:?}"));
    let materials = load()?;
    let software = LevelRenderer::from_level(&level(name)?, materials, &BuildConfig::default())
        .map_err(|e| format!("{e:?}"))?;
    let gpu = Gpu::new().map_err(|e| e.to_string())?;
    let (table, enemies, lights) = crowd(&software, &load()?, eye)?;
    let assets = software.assets();
    let finish = Finish::of(quality, &gpu);
    let start = Instant::now();
    let mut renderer =
        GpuRenderer::with_finish(gpu.clone(), assets, finish).map_err(|e| e.to_string())?;
    let upload = start.elapsed().as_secs_f64() * 1000.0;
    let word = |w: &str| std::env::args().any(|a| a == w);
    let effects = match (word("plain"), word("noshadows")) {
        (true, _) => Some(GpuEffects::OFF),
        (_, true) => Some(GpuEffects {
            shadows: false,
            ..GpuEffects::at(quality)
        }),
        _ => None,
    };
    renderer.set_effects(effects);
    let (mut cpu, mut done) = (vec![], vec![]);
    for i in 0..FRAMES {
        let yaw = i as f32 / FRAMES as f32 * std::f32::consts::TAU;
        let view = View::from_angles(eye, yaw, -0.1, 0.0);
        let frame = FrameInput {
            view: &view,
            quality,
            actors: assets.actors(),
            effects: Effects {
                lights: &lights,
                models: Some((&table, &enemies)),
                ..Effects::default()
            },
            wants_depth: false,
        };
        let t = Instant::now();
        renderer
            .draw_unread(assets, SIZE, &frame)
            .ok_or("no frame")?;
        cpu.push(t.elapsed().as_secs_f64() * 1000.0);
        gpu.wait();
        done.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    if let Some(error) = renderer.take_error() {
        return Err(error);
    }
    // The first frame makes the targets and compiles what is left, so it is
    // given apart from the rest.
    let (first_cpu, first_done) = (cpu.remove(0), done.remove(0));
    let (c, d) = (spread(cpu), spread(done));
    println!(
        "{name} {quality:?} {finish:?}: upload {upload:.1} ms, first frame {first_cpu:.2} CPU, {first_done:.2} done"
    );
    println!(
        "  CPU side median {:.2} p95 {:.2} worst {:.2} ms",
        c.0, c.1, c.2
    );
    println!(
        "  done     median {:.2} p95 {:.2} worst {:.2} ms",
        d.0, d.1, d.2
    );
    Ok(())
}

fn main() -> Result<(), String> {
    println!(
        "{}x{}, {FRAMES} frames turning a full circle",
        SIZE.0, SIZE.1
    );
    let levels = [
        ("showcase", Vec3::new(3.0, 3.0, 2.5)),
        ("facility", Vec3::new(-12.0, 0.0, 1.5)),
        ("voxel-cave", Vec3::new(8.0, 8.0, 4.0)),
    ];
    for (name, eye) in levels {
        for quality in [Quality::High, Quality::Medium] {
            measure(name, eye, quality)?;
        }
    }
    Ok(())
}

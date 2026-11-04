//! The stock sentry and the player's ship in a room lit by one static lamp, drawn on
//! the GPU from the same view with the shadows off and on, to look at:
//! `cargo run --release -p mscent-gpu --example lamp_shot -- out_dir [width height]`
//! writes `lamp_before.png` and `lamp_after.png` into `out_dir`. Nothing here is a
//! dynamic light: the lamp is in the level and baked, and what moves shades it.
//! With `time` after the size, it also prints what each frame costs until the GPU is
//! done.

use std::path::Path;
use std::time::Instant;

use mscent_gpu::{Finish, Gpu, GpuEffects, GpuRenderer};
use mscent_math::{Quat, Vec3, Vec3d};
use mscent_render::model::{ModelInstance, ModelTable};
use mscent_render::png_encode::encode_png;
use mscent_render::{
    BuildConfig, Effects, FrameInput, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::materials::ROCK;
use mscent_world::model::{build, read};
use mscent_world::{Brush, Entity, Level, Op};

/// Frames timed with `time` after the size.
const FRAMES: usize = 120;
const SENTRY: &str = include_str!("../../../assets/enemies/sentry.msb");
const SHIP: &str = include_str!("../../../assets/ship/player.msb");

/// A room 24 wide and 28 long of rock, with a lamp over its middle.
fn room() -> Result<Level, String> {
    let v = |x, y, z| Vec3d::new(x, y, z);
    let shell = Brush::cuboid(v(-12.0, -12.0, -4.0), v(12.0, 16.0, 9.0), Op::Subtract, 0)
        .map_err(|e| format!("{e:?}"))?;
    Ok(Level {
        brushes: vec![shell.with_material(ROCK)],
        entities: vec![Entity::Light {
            pos: Vec3::new(0.0, 2.0, 6.0),
            color: Vec3::new(1.0, 0.9, 0.75),
            radius: 40.0,
            intensity: 0.5,
        }],
        ..Level::default()
    })
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = args
        .first()
        .ok_or("usage: lamp_shot <out_dir> [width height]")?;
    let side = |i: usize, default: u32| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(default);
    let (width, height) = (side(1, 1280), side(2, 720));
    let textures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let load = || MaterialTable::load_dir(&textures).map_err(|e| format!("{e:?}"));
    let level = LevelRenderer::from_level(&room()?, load()?, &BuildConfig::default())
        .map_err(|e| format!("{e:?}"))?;
    let mut table = ModelTable::new();
    let mut add = |text: &str| -> Result<_, String> {
        let blueprint = read(text.as_bytes()).map_err(|e| format!("{e:?}"))?;
        let model = build(&blueprint).map_err(|e| format!("{e:?}"))?;
        Ok(table.add(&blueprint, &model, &load()?))
    };
    let (sentry, ship) = (add(SENTRY)?, add(SHIP)?);
    let up = Vec3::new(0.0, 0.0, 1.0);
    let facing = View::from_angles(Vec3::ZERO, 0.0, 0.0, 0.0).orientation;
    let things = [
        ModelInstance {
            scale: 1.6,
            ..ModelInstance::new(
                sentry,
                Vec3::new(-1.5, 3.0, -1.5),
                Quat::from_axis_angle(up, 0.6),
            )
        },
        ModelInstance {
            scale: 1.5,
            ..ModelInstance::new(ship, Vec3::new(2.5, 1.0, -2.2), facing)
        },
    ];
    let gpu = Gpu::new().map_err(|e| e.to_string())?;
    let timer = gpu.clone();
    println!("adapter: {}", gpu.adapter());
    let finish = Finish::of(Quality::High, &gpu);
    let mut renderer =
        GpuRenderer::with_finish(gpu, level.assets(), finish).map_err(|e| e.to_string())?;
    let view = View::from_angles(Vec3::new(0.5, -9.0, 1.5), -0.05, -0.42, 0.0);
    let mut fb = Framebuffer::new(width, height).ok_or("bad size")?;
    let frame = FrameInput {
        view: &view,
        quality: Quality::High,
        actors: &[],
        effects: Effects {
            models: Some((&table, &things)),
            ..Effects::default()
        },
        wants_depth: false,
    };
    let off = GpuEffects {
        shadows: false,
        ..GpuEffects::at(Quality::High)
    };
    for (name, effects) in [
        ("lamp_before.png", off),
        ("lamp_after.png", GpuEffects::at(Quality::High)),
    ] {
        renderer.set_effects(Some(effects));
        renderer
            .draw(level.assets(), &mut fb, &frame)
            .ok_or("no frame")?;
        if let Some(error) = renderer.take_error() {
            return Err(error);
        }
        let png = encode_png(width, height, fb.pixels()).ok_or("cannot encode")?;
        std::fs::write(Path::new(dir).join(name), png).map_err(|e| e.to_string())?;
        println!("wrote {name}");
        if args.iter().any(|a| a == "time") {
            let mut costs = Vec::new();
            for _ in 0..FRAMES {
                let start = Instant::now();
                renderer
                    .draw_unread(level.assets(), (width, height), &frame)
                    .ok_or("no frame")?;
                timer.wait();
                costs.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            costs.sort_by(f64::total_cmp);
            println!("  {name}: median {:.2} ms until done", costs[FRAMES / 2]);
        }
    }
    Ok(())
}

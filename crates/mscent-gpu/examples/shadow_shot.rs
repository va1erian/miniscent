//! An enemy model near a dynamic light in a lit room, drawn on the GPU from the same
//! view with the shadows off and on, to look at:
//! `cargo run --release -p mscent-gpu --example shadow_shot -- out_dir [width height]`
//! writes `shadow_before.png` and `shadow_after.png` into `out_dir`. The enemy is the
//! stock sentry, floating above the floor between the light and the far wall, so that
//! its shadow falls on both.

use std::path::Path;

use mscent_gpu::{Finish, Gpu, GpuEffects, GpuRenderer};
use mscent_math::{Quat, Vec3, Vec3d};
use mscent_render::model::{ModelInstance, ModelTable};
use mscent_render::png_encode::encode_png;
use mscent_render::{
    BuildConfig, DynLight, Effects, FrameInput, Framebuffer, LevelRenderer, MaterialTable, Quality,
    View,
};
use mscent_world::materials::ROCK;
use mscent_world::model::{build, read};
use mscent_world::{Brush, Entity, Level, Op};

const SENTRY: &str = include_str!("../../../assets/enemies/sentry.msb");

/// A room 24 wide and 28 long of rock, with a dim bake so
/// that what the dynamic light does reads against it.
fn room() -> Result<Level, String> {
    let v = |x, y, z| Vec3d::new(x, y, z);
    let shell = Brush::cuboid(v(-12.0, -12.0, -4.0), v(12.0, 16.0, 9.0), Op::Subtract, 0)
        .map_err(|e| format!("{e:?}"))?;
    Ok(Level {
        brushes: vec![shell.with_material(ROCK)],
        entities: vec![Entity::Light {
            pos: Vec3::new(0.0, 2.0, 7.0),
            color: Vec3::new(0.8, 0.9, 1.0),
            radius: 40.0,
            intensity: 0.06,
        }],
        ..Level::default()
    })
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = args
        .first()
        .ok_or("usage: shadow_shot <out_dir> [width height]")?;
    let side = |i: usize, default: u32| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(default);
    let (width, height) = (side(1, 1280), side(2, 720));
    let textures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let load = || MaterialTable::load_dir(&textures).map_err(|e| format!("{e:?}"));
    let level = LevelRenderer::from_level(&room()?, load()?, &BuildConfig::default())
        .map_err(|e| format!("{e:?}"))?;
    let blueprint = read(SENTRY.as_bytes()).map_err(|e| format!("{e:?}"))?;
    let model = build(&blueprint).map_err(|e| format!("{e:?}"))?;
    let mut table = ModelTable::new();
    let id = table.add(&blueprint, &model, &load()?);
    let turn = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), 0.6);
    let enemies = [ModelInstance {
        scale: 1.6,
        ..ModelInstance::new(id, Vec3::new(0.0, 2.0, -1.2), turn)
    }];
    let mut light = DynLight::new();
    let (at, warm) = (Vec3::new(-3.5, -2.5, 2.5), Vec3::new(6.0, 4.6, 2.6));
    if !light.place(level.scene(), at, 30.0, warm) {
        return Err("the light cannot be placed".into());
    }
    let lights = [light];
    let gpu = Gpu::new().map_err(|e| e.to_string())?;
    println!("adapter: {}", gpu.adapter());
    let finish = Finish::of(Quality::High, &gpu);
    let mut renderer =
        GpuRenderer::with_finish(gpu, level.assets(), finish).map_err(|e| e.to_string())?;
    let view = View::from_angles(Vec3::new(1.0, -9.0, 1.5), -0.12, -0.42, 0.0);
    let mut fb = Framebuffer::new(width, height).ok_or("bad size")?;
    let frame = FrameInput {
        view: &view,
        quality: Quality::High,
        actors: &[],
        effects: Effects {
            lights: &lights,
            models: Some((&table, &enemies)),
            ..Effects::default()
        },
        wants_depth: false,
    };
    for (name, effects) in [
        // Parallax stays on in both, so that the shadows are all that differs.
        (
            "shadow_before.png",
            GpuEffects {
                shadows: false,
                ..GpuEffects::at(Quality::High)
            },
        ),
        ("shadow_after.png", GpuEffects::at(Quality::High)),
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
    }
    Ok(())
}

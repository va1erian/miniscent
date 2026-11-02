//! The gate that keeps the two renderers on par (`docs/hardware-renderer.md`).
//! Each case draws one frame on both, from current code, with nothing stored.
//! It must also fail with its feature taken out of the software frame, or it is
//! not looking at that feature.
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set, as CI
//! sets it.

#[path = "support/compare.rs"]
mod compare;
#[path = "support/scenes.rs"]
mod scenes;

use std::path::{Path, PathBuf};

use compare::{Frame, compare};
use mscent_gpu::{Gpu, GpuError, GpuRenderer};
use mscent_render::png_encode::encode_png;
use mscent_render::{Effects, FrameInput, Framebuffer, LevelRenderer, Quality};
use scenes::{Framing, Pose, Scene, Taken};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;

/// A case: what to draw, and what to take out to prove it can fail.
struct Case {
    name: &'static str,
    pose: Pose,
    quality: Quality,
    without: Taken,
}

fn cases() -> Vec<Case> {
    use Pose::*;
    use Quality::*;
    let case = |name, pose, quality, without| Case {
        name,
        pose,
        quality,
        without,
    };
    vec![
        case("sky", OutsideTheLevel, High, Taken::SkyColors),
        case("fog", FarFromMaterial("floor"), Low, Taken::Fog),
        case("texture", AtMaterial("brick"), Low, Taken::Textures),
        case("mips", FarFromMaterial("floor"), Low, Taken::Textures),
        case("lightmap", DownTheHall, Low, Taken::Lamps),
        case("light_mips", AtTheFloor, Low, Taken::Lamps),
        case("emissive", AtMaterial("lightpanel"), High, Taken::Textures),
        case("bump", AtMaterial("brick"), Medium, Taken::Quality(Low)),
        case(
            "specular",
            AtMaterial("metal"),
            High,
            Taken::Quality(Medium),
        ),
        case("actor", AtActor, High, Taken::Actors),
        case("mover", AtDoor, High, Taken::MoverPose),
    ]
}

fn software(scene: &Scene, frame: &Framing, quality: Quality, without: Option<Taken>) -> Frame {
    let renderer = scene.software(without);
    let mut fb = Framebuffer::new(WIDTH, HEIGHT).expect("a framebuffer");
    let quality = match without {
        Some(Taken::Quality(lower)) => lower,
        _ => quality,
    };
    let actors = match without {
        Some(Taken::Actors) => &[][..],
        _ => &frame.actors[..],
    };
    let poses = match without {
        Some(Taken::MoverPose) => &[][..],
        _ => &frame.poses[..],
    };
    let effects = Effects {
        poses,
        ..Effects::default()
    };
    renderer
        .render_with_effects(&mut fb, &frame.view, quality, actors, None, &effects)
        .expect("a frame");
    Frame::of(&fb)
}

fn hardware(on_gpu: &mut OnGpu, frame: &Framing, case: &Case) -> Frame {
    let mut fb = Framebuffer::new(WIDTH, HEIGHT).expect("a framebuffer");
    let input = FrameInput {
        view: &frame.view,
        quality: case.quality,
        actors: &frame.actors,
        effects: Effects {
            poses: &frame.poses,
            ..Effects::default()
        },
        wants_depth: true,
    };
    let OnGpu { level, renderer } = on_gpu;
    renderer
        .draw(level.assets(), &mut fb, &input)
        .expect("a frame");
    if let Some(error) = renderer.take_error() {
        panic!("{}: the GPU reported: {error}", case.name);
    }
    Frame::of(&fb)
}

/// Where a failed case leaves its pictures to be looked at.
fn write_pictures(name: &str, reference: &Frame, other: &Frame) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("parity");
    std::fs::create_dir_all(&dir).expect("a directory for the pictures");
    let amplified: Vec<u32> = (reference.pixels.iter().zip(&other.pixels))
        .map(|(&a, &b)| {
            let channel = |s: u32| {
                let error = ((a >> s) & 255).abs_diff((b >> s) & 255);
                (error * 8).min(255) << s
            };
            0xff00_0000 | channel(16) | channel(8) | channel(0)
        })
        .collect();
    let pictures = [
        ("sw", &reference.pixels),
        ("hw", &other.pixels),
        ("diff", &amplified),
    ];
    for (kind, pixels) in pictures {
        let png = encode_png(WIDTH, HEIGHT, pixels).expect("a PNG");
        std::fs::write(dir.join(format!("{name}-{kind}.png")), png).expect("a written PNG");
    }
    dir
}

/// The GPU, or `None` where a skip is allowed.
fn gpu() -> Option<Gpu> {
    match Gpu::new() {
        Ok(gpu) => Some(gpu),
        Err(why @ (GpuError::NoAdapter(_) | GpuError::NoDevice(_))) => {
            assert!(
                std::env::var_os("MSCENT_REQUIRE_GPU").is_none(),
                "MSCENT_REQUIRE_GPU is set: {why}"
            );
            eprintln!("skipped: {why}");
            None
        }
        Err(why) => panic!("{why}"),
    }
}

/// A level, and what of it is on the GPU.
struct OnGpu {
    level: LevelRenderer,
    renderer: GpuRenderer,
}

fn on_gpu(gpu: Gpu, scene: &Scene) -> OnGpu {
    let level = scene.software(None);
    let renderer = GpuRenderer::new(gpu, level.assets()).expect("the level fits on the GPU");
    OnGpu { level, renderer }
}

#[test]
fn the_renderers_draw_the_same_frames() {
    let Some(first) = gpu() else { return };
    let (second, third) = (first.clone(), first.clone());
    eprintln!("adapter: {}", first.adapter());
    let (showcase, door, hard) = (Scene::showcase(), Scene::door(), Scene::hard_shadows());
    let mut showcase_gpu = on_gpu(first, &showcase);
    let mut door_gpu = on_gpu(second, &door);
    let mut hard_gpu = on_gpu(third, &hard);
    let mut failures = Vec::new();
    for case in cases() {
        let (scene, renderer) = match (case.name, case.pose) {
            (_, Pose::AtDoor) => (&door, &mut door_gpu),
            ("light_mips", _) => (&hard, &mut hard_gpu),
            _ => (&showcase, &mut showcase_gpu),
        };
        let framing = scene.framing(case.pose);
        let reference = software(scene, &framing, case.quality, None);
        let drawn = hardware(renderer, &framing, &case);
        let report = compare(&reference, &drawn);
        eprintln!("{}: {report:?}", case.name);
        if !report.alike() {
            let dir = write_pictures(case.name, &reference, &drawn);
            let where_ = dir.display();
            failures.push(format!("{}: {report:?}, pictures in {where_}", case.name));
        }
        let stripped = software(scene, &framing, case.quality, Some(case.without));
        let report = compare(&reference, &stripped);
        if report.alike() {
            failures.push(format!("{}: cannot fail, {report:?}", case.name));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_sky_case_sees_only_sky() {
    let showcase = Scene::showcase();
    let framing = showcase.framing(Pose::OutsideTheLevel);
    let frame = software(&showcase, &framing, Quality::High, None);
    assert!(frame.depth.iter().all(|&d| d == 0.0));
}

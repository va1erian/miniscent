//! The gate that keeps the two renderers on par (`docs/hardware-renderer.md`).
//! Each case draws one frame on both, from current code, with nothing stored.
//! It must also fail with its feature taken out of the software frame, or it is
//! not looking at that feature.
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set, as CI
//! sets it.
//!
//! The hardware renderer draws flat faces so far, so the reference is
//! `Scene::render`, the flat-lit software path. It becomes the real frame when
//! the shading arrives.

#[path = "support/compare.rs"]
mod compare;

use std::path::{Path, PathBuf};

use compare::{Frame, compare};
use mscent_gpu::{Gpu, GpuError, GpuRenderer};
use mscent_math::Vec3;
use mscent_render::png_encode::encode_png;
use mscent_render::{
    BuildConfig, Effects, FrameInput, FrameRenderer, Framebuffer, LevelAssets, LevelRenderer,
    Material, MaterialTable, Quality, Texture, View,
};
use mscent_world::{materials, showcase_level};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;

/// What a case leaves out of its software frame to prove it can fail.
#[derive(Clone, Copy)]
enum Without {
    /// The gradient, for one flat colour.
    Sky,
    /// The level's faces, leaving the sky.
    Faces,
}

struct Case {
    name: &'static str,
    eye: [f32; 3],
    yaw: f32,
    pitch: f32,
    without: Without,
}

const CASES: &[Case] = &[
    // Far outside the level and looking away, where only the sky is.
    Case {
        name: "sky",
        eye: [-500.0, 0.0, 0.0],
        yaw: std::f32::consts::PI,
        pitch: 0.0,
        without: Without::Sky,
    },
    Case {
        name: "faces_room",
        eye: [3.0, 3.0, 2.5],
        yaw: -0.78,
        pitch: 0.0,
        without: Without::Faces,
    },
    Case {
        name: "faces_floor",
        eye: [36.0, 4.0, 3.0],
        yaw: 0.5,
        pitch: -0.8,
        without: Without::Faces,
    },
    Case {
        name: "faces_through_a_door",
        eye: [18.0, 40.0, 3.0],
        yaw: 1.6,
        pitch: 0.1,
        without: Without::Faces,
    },
];

/// Every material one flat colour, as the flat faces have no texture yet. Not a
/// grey: a grey would not see the channels swapped.
fn flat_materials() -> MaterialTable {
    let mut table = MaterialTable::new();
    for name in materials::NAMES {
        table.add(Material {
            name: name.to_string(),
            gloss: 0,
            texture: Texture::solid(0xff40_80c0),
        });
    }
    table
}

/// The showcase without spending time on light, which nothing here reads yet.
fn level() -> LevelAssets {
    let cfg = BuildConfig {
        light_radius: 0.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    LevelRenderer::from_level(&showcase_level(), flat_materials(), &cfg)
        .expect("the showcase builds")
        .into_assets()
}

fn view_of(case: &Case) -> View {
    let [x, y, z] = case.eye;
    View::from_angles(Vec3::new(x, y, z), case.yaw, case.pitch, 0.0)
}

fn software(assets: &LevelAssets, case: &Case, leave_out: Option<Without>) -> Frame {
    let mut fb = Framebuffer::new(WIDTH, HEIGHT).expect("a framebuffer");
    let cfg = assets.config();
    let cam = assets
        .camera(&view_of(case), WIDTH, HEIGHT)
        .expect("a camera");
    match leave_out {
        Some(Without::Sky) => fb.clear(0xff80_8080),
        _ => fb.clear_sky(cfg.sky_top, cfg.sky_bottom),
    }
    if !matches!(leave_out, Some(Without::Faces)) {
        assets.scene().render(&mut fb, &cam, None);
    }
    Frame::of(&fb)
}

fn hardware(renderer: &mut GpuRenderer, case: &Case) -> Frame {
    let mut fb = Framebuffer::new(WIDTH, HEIGHT).expect("a framebuffer");
    let input = FrameInput {
        view: &view_of(case),
        quality: Quality::High,
        actors: &[],
        effects: Effects::default(),
    };
    renderer.draw(&mut fb, &input).expect("a frame");
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

#[test]
fn the_renderers_draw_the_same_frames() {
    let Some(gpu) = gpu() else { return };
    eprintln!("adapter: {}", gpu.adapter());
    let reference_assets = level();
    let mut renderer = GpuRenderer::new(gpu, level())
        .map_err(|(why, _)| why)
        .expect("the showcase fits on the GPU");
    let mut failures = Vec::new();
    for case in CASES {
        let reference = software(&reference_assets, case, None);
        let drawn = hardware(&mut renderer, case);
        let report = compare(&reference, &drawn);
        eprintln!("{}: {report:?}", case.name);
        if !report.alike() {
            let dir = write_pictures(case.name, &reference, &drawn);
            failures.push(format!(
                "{}: {report:?}, pictures in {}",
                case.name,
                dir.display()
            ));
        }
        let stripped = software(&reference_assets, case, Some(case.without));
        let report = compare(&reference, &stripped);
        if report.alike() {
            failures.push(format!("{}: cannot fail, {report:?}", case.name));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_sky_case_sees_only_sky() {
    let assets = level();
    let frame = software(&assets, &CASES[0], None);
    assert!(frame.depth.iter().all(|&d| d == 0.0));
}

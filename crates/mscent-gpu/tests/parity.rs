//! The gate that keeps the two renderers on par (`docs/hardware-renderer.md`).
//! Each case draws one frame on both, from current code, with nothing stored.
//! It must also fail with its feature taken out of the software frame, or it is
//! not looking at that feature.
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set, as CI
//! sets it.

#[path = "support/compare.rs"]
mod compare;
#[path = "support/gpu.rs"]
mod gpu;
#[path = "support/particles.rs"]
mod particles;
#[path = "support/scenes.rs"]
mod scenes;

use std::path::{Path, PathBuf};

use compare::{Frame, compare};
use gpu::gpu;
use mscent_gpu::{Gpu, GpuRenderer};
use mscent_math::Quat;
use mscent_math::Vec3;
use mscent_render::model::ModelInstance;
use mscent_render::particles::SpriteBank;
use mscent_render::png_encode::encode_png;
use mscent_render::{Effects, FrameInput, Framebuffer, LevelRenderer, Quality};
use particles::Sparks;
use scenes::{Framing, Pose, Scene, Taken, models};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;

/// A case: what to draw, and what to take out to prove it can fail.
struct Case {
    name: &'static str,
    pose: Pose,
    quality: Quality,
    without: Taken,
    /// How far ahead of the eye a dynamic light stands, if the case has one.
    light: Option<f32>,
    sparks: Option<Sparks>,
    /// How far ahead of the eye enemy models stand, if the case has them.
    models: Option<f32>,
}

impl Case {
    fn models(self, ahead: f32) -> Self {
        Self {
            models: Some(ahead),
            ..self
        }
    }

    fn with(self, sparks: Sparks) -> Self {
        Self {
            sparks: Some(sparks),
            ..self
        }
    }

    fn lit(self, ahead: f32) -> Self {
        Self {
            light: Some(ahead),
            ..self
        }
    }
}

fn cases() -> Vec<Case> {
    use Pose::*;
    use Quality::*;
    let case = |name, pose, quality, without| Case {
        name,
        pose,
        quality,
        without,
        light: None,
        sparks: None,
        models: None,
    };
    let metal = Scene::showcase().surface("metal");
    let decal = Sparks::Decal {
        centre: metal.0,
        normal: metal.1,
    };
    vec![
        case("sky", OutsideTheLevel, High, Taken::SkyColors),
        case("fog", FarFromMaterial("floor"), Low, Taken::Fog),
        case("texture", AtMaterial("brick"), Low, Taken::Textures),
        case("mips", FarFromMaterial("metal"), Low, Taken::Textures),
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
        case("mover_turned", AtTurnedDoor, High, Taken::MoverPose),
        case("sprite_add", DownTheHall, Low, Taken::Particles).with(Sparks::Added),
        case("sprite_over", DownTheHall, Low, Taken::Particles).with(Sparks::Over),
        case("streak", DownTheHall, Low, Taken::Particles).with(Sparks::Streaks),
        case("decal", AtMaterial("metal"), High, Taken::Particles).with(decal),
        case("dyn_light", AtMaterial("metal"), Low, Taken::DynLights).lit(1.5),
        case(
            "dyn_light_bump",
            AtMaterial("metal"),
            Medium,
            Taken::DynLights,
        )
        .lit(1.5),
        case(
            "dyn_light_specular",
            AtMaterial("metal"),
            High,
            Taken::DynLights,
        )
        .lit(1.5),
        case("dyn_light_actor", AtActor, High, Taken::DynLights).lit(2.5),
        case(
            "voxel",
            look(0.0, -6.0, 1.0, 0.0, 5.0, -2.0),
            Low,
            Taken::Voxels,
        ),
        case(
            "voxel_shadow",
            look(0.5, -0.5, 4.0, 0.0, 4.5, 0.0),
            Low,
            Taken::Lamps,
        ),
        case(
            "voxel_bump",
            look(2.5, 0.5, 0.5, 0.0, 5.0, -1.0),
            Medium,
            Taken::Quality(Low),
        ),
        case(
            "voxel_specular",
            look(4.0, 3.5, 0.5, 3.6, 7.5, -2.0),
            High,
            Taken::Quality(Medium),
        ),
        case(
            "voxel_blend",
            look(2.5, 3.0, 0.0, 2.0, 6.5, -2.0),
            High,
            Taken::Textures,
        ),
        case(
            "voxel_spots",
            look(-2.0, 2.0, 0.5, -4.5, 6.0, -2.0),
            High,
            Taken::Spots,
        ),
        case(
            "voxel_lod",
            look(0.0, -18.0, 2.0, 0.0, 6.0, -2.0),
            Low,
            Taken::Voxels,
        ),
        case(
            "voxel_dyn_light",
            look(0.0, -6.0, 1.0, 0.0, 5.0, -2.0),
            Low,
            Taken::DynLights,
        )
        .lit(7.5),
        case(
            "voxel_mips",
            look(6.5, -14.0, 0.0, 6.5, 2.0, -2.75),
            Low,
            Taken::Textures,
        ),
        case("vista", AtVista, High, Taken::Vistas),
        case("vista_parallax", AtVistaAside, Low, Taken::Parallax),
        case("vista_spin", AtVista, Low, Taken::Spin),
        case("vista_pan", AtVista, Low, Taken::Pan),
        case("vista_fog", AtVista, Low, Taken::VistaFog),
        case("cutout", AtVista, Low, Taken::Cutouts),
        case("vista_voxels", AtVista, Low, Taken::Voxels),
        case("model", AtModels { ahead: 3.0 }, Low, Taken::Models).models(3.0),
        case(
            "model_texture",
            AtModels { ahead: 3.0 },
            High,
            Taken::Textures,
        )
        .models(3.0),
        case(
            "model_turned",
            AtModels { ahead: 3.0 },
            Low,
            Taken::ModelTurns,
        )
        .models(3.0),
        // A light before the probes, so that their fronts catch bump and highlight.
        case(
            "model_bump",
            AtModels { ahead: 2.0 },
            Medium,
            Taken::Quality(Low),
        )
        .models(2.0)
        .lit(1.0),
        case(
            "model_specular",
            AtModels { ahead: 3.0 },
            High,
            Taken::Quality(Medium),
        )
        .models(3.0)
        .lit(1.5),
        // In the long voxel room, whose far wall is square on.
        case(
            "model_lod",
            look(0.0, -18.0, 2.0, 0.0, 6.0, -2.0),
            Low,
            Taken::Models,
        )
        .models(14.0),
        case(
            "model_dyn_light",
            AtModels { ahead: 3.0 },
            Low,
            Taken::DynLights,
        )
        .models(3.0)
        .lit(2.0),
    ]
}

/// Standing at the first three, looking at the last three.
fn look(x: f32, y: f32, z: f32, ax: f32, ay: f32, az: f32) -> Pose {
    Pose::Looking {
        eye: Vec3::new(x, y, z),
        at: Vec3::new(ax, ay, az),
    }
}

/// The sprite masks, made once.
fn bank() -> &'static SpriteBank {
    static BANK: std::sync::OnceLock<SpriteBank> = std::sync::OnceLock::new();
    BANK.get_or_init(SpriteBank::new)
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
    let lights = match without {
        Some(Taken::DynLights) => &[][..],
        _ => &frame.lights[..],
    };
    let particles = match without {
        Some(Taken::Particles) => None,
        _ => frame.particles.as_ref().map(|p| (p, bank())),
    };
    let turned: Vec<ModelInstance>;
    let instances = match without {
        Some(Taken::Models) => &[][..],
        Some(Taken::ModelTurns) => {
            let still = |m: &ModelInstance| ModelInstance {
                rot: Quat::IDENTITY,
                ..*m
            };
            turned = frame.models.iter().map(still).collect();
            &turned[..]
        }
        _ => &frame.models[..],
    };
    let effects = Effects {
        poses,
        lights,
        particles,
        vista_time: frame.time,
        models: Some((&models::table().0, instances)),
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
            lights: &frame.lights,
            particles: frame.particles.as_ref().map(|p| (p, bank())),
            vista_time: frame.time,
            models: Some((&models::table().0, &frame.models)),
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
    let (voxel_room, voxel_distance) = (Scene::voxel_room(), Scene::voxel_distance());
    let vista = Scene::vista();
    let mut showcase_gpu = on_gpu(first.clone(), &showcase);
    let mut door_gpu = on_gpu(second, &door);
    let mut hard_gpu = on_gpu(third, &hard);
    let mut vista_gpu = on_gpu(first.clone(), &vista);
    let mut failures = Vec::new();
    for case in cases() {
        // A voxel frame's detail depends on the frames before it, as software's
        // does, so each voxel case starts a level of its own on both.
        let mut voxel_gpu;
        let (scene, renderer) = match (case.name, case.pose) {
            (_, Pose::AtDoor | Pose::AtTurnedDoor) => (&door, &mut door_gpu),
            ("vista_voxels", _) => {
                voxel_gpu = on_gpu(first.clone(), &vista);
                (&vista, &mut voxel_gpu)
            }
            (_, Pose::AtVista | Pose::AtVistaAside) => (&vista, &mut vista_gpu),
            ("light_mips", _) => (&hard, &mut hard_gpu),
            // A model's detail also depends on the frames before it.
            (name, _) if name.starts_with("model") => {
                let scene = match name {
                    "model_lod" => &voxel_distance,
                    _ => &showcase,
                };
                voxel_gpu = on_gpu(first.clone(), scene);
                (scene, &mut voxel_gpu)
            }
            (name, _) if name.starts_with("voxel") => {
                let scene = match name {
                    "voxel_lod" | "voxel_mips" => &voxel_distance,
                    _ => &voxel_room,
                };
                voxel_gpu = on_gpu(first.clone(), scene);
                (scene, &mut voxel_gpu)
            }
            _ => (&showcase, &mut showcase_gpu),
        };
        let mut framing = scene.framing(case.pose);
        if let Some(ahead) = case.models {
            framing.models = models::ahead_of(&framing.view, ahead);
        }
        if let Some(ahead) = case.light {
            scene.light(&mut framing, ahead);
        }
        if let Some(kind) = case.sparks {
            let level = scene.software(None);
            let cam = level.assets().camera(&framing.view, WIDTH, HEIGHT);
            let cam = cam.expect("a camera");
            framing.particles = Some(particles::draws(kind, &cam, &framing.view));
        }
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
            // The "hw" picture is the stripped software frame.
            let dir = write_pictures(&format!("{}-stripped", case.name), &reference, &stripped);
            let where_ = dir.display();
            failures.push(format!(
                "{}: cannot fail, {report:?}, pictures in {where_}",
                case.name
            ));
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

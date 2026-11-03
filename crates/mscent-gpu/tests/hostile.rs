//! Hostile levels, views and frames through the GPU build and draw: each gives
//! an error or a frame, never a panic (debug builds also trap overflow) and never
//! a wgpu error nobody asked about. The inputs are those of `mscent-render`'s
//! `tests/hostile_level.rs`, and the sizes a GPU adds: large atlases, many faces,
//! materials and movers, dense voxels, degenerate brushes and odd frames.

#[path = "support/gpu.rs"]
mod gpu;

use std::path::Path;

use mscent_gpu::{Finish, Gpu, GpuError, GpuRenderer};
use mscent_math::{DPlane, Quat, Vec3, Vec3d};
use mscent_render::camera::MAX_SIDE;
use mscent_render::{
    Actor, ActorKind, BuildConfig, Effects, FrameInput, Framebuffer, LevelRenderer, MaterialTable,
    MoverPose, Quality, View,
};
use mscent_world::voxel::{CellSize, Context, Volume, fill};
use mscent_world::{Entity, Level, LevelBuilder, MoverGroup, Op};

const NAN: f32 = f32::NAN;

fn materials() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("textures")
}

/// A quick bake: these levels are about sizes, not light.
fn quick() -> BuildConfig {
    BuildConfig {
        luxel_size: 1.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    }
}

fn light() -> Entity {
    Entity::Light {
        pos: Vec3::new(5.0, 5.0, 4.0),
        color: Vec3::new(1.0, 1.0, 1.0),
        radius: 12.0,
        intensity: 1.0,
    }
}

fn room() -> Level {
    let mut b = LevelBuilder::new();
    b.room([0.0, 0.0, 0.0], [10.0, 10.0, 6.0], (0, 1, 2));
    Level {
        brushes: b.finish(),
        entities: vec![light()],
        ..Default::default()
    }
}

fn eye() -> Vec3 {
    Vec3::new(2.0, 2.0, 2.0)
}

fn plain_view() -> View {
    View::from_angles(eye(), 0.7, -0.2, 0.0)
}

/// A frame of each quality, drawn and read back with depth.
fn draw_all(
    renderer: &mut GpuRenderer,
    level: &LevelRenderer,
    fb: &mut Framebuffer,
    view: &View,
    actors: &[Actor],
    poses: &[MoverPose],
) {
    for quality in [Quality::Low, Quality::Medium, Quality::High] {
        let input = FrameInput {
            view,
            quality,
            actors,
            effects: Effects {
                poses,
                ..Effects::default()
            },
            wants_depth: true,
        };
        let _ = renderer.draw(level.assets(), fb, &input);
        assert_eq!(renderer.take_error(), None, "{view:?} {quality:?}");
    }
}

/// `level` on the GPU in both finishes, or `None` where it does not fit. Any
/// error but a size the checks found is a check missing.
fn on_gpu(device: &Gpu, level: &LevelRenderer) -> Vec<GpuRenderer> {
    let mut made = Vec::new();
    for finish in [Finish::Reference, Finish::Fine] {
        match GpuRenderer::with_finish(device.clone(), level.assets(), finish) {
            Ok(renderer) => made.push(renderer),
            Err(GpuError::TooLarge(what)) => eprintln!("refused: {what}"),
            Err(other) => panic!("{other}"),
        }
        assert_eq!(device.take_error(), None);
    }
    made
}

/// Builds `level` in software, then on the GPU, and draws it from `views`.
fn through_the_gpu(device: &Gpu, level: &Level, cfg: &BuildConfig, views: &[View]) {
    let Ok(software) = LevelRenderer::from_level(level, materials(), cfg) else {
        return;
    };
    let mut fb = Framebuffer::new(64, 48).expect("a framebuffer");
    for mut renderer in on_gpu(device, &software) {
        for view in views {
            draw_all(&mut renderer, &software, &mut fb, view, &[], &[]);
        }
    }
}

#[test]
fn empty_and_light_free_levels_build_and_draw() {
    let Some(device) = gpu::gpu() else { return };
    let dark = Level {
        entities: vec![],
        ..room()
    };
    for level in [Level::default(), dark] {
        through_the_gpu(&device, &level, &quick(), &[plain_view()]);
    }
}

#[test]
fn hostile_lights_and_build_configs_build_and_draw() {
    let Some(device) = gpu::gpu() else { return };
    let lights = [
        (Vec3::new(NAN, 0.0, 0.0), 5.0, 1.0),
        (eye(), -3.0, 1.0),
        (eye(), f32::INFINITY, 1.0),
        (eye(), 5.0, f32::MAX),
        (eye(), 5.0, NAN),
        (Vec3::new(1.0e30, 0.0, 0.0), 5.0, 1.0),
    ];
    for (pos, radius, intensity) in lights {
        let level = Level {
            entities: vec![Entity::Light {
                pos,
                color: Vec3::new(1.0, 1.0, 1.0),
                radius,
                intensity,
            }],
            ..room()
        };
        through_the_gpu(&device, &level, &quick(), &[plain_view()]);
    }
    let base = quick();
    let configs = [
        BuildConfig {
            luxel_size: NAN,
            ..base
        },
        BuildConfig {
            luxel_size: -1.0,
            ..base
        },
        // The finest bake the baker takes: the largest atlas a room can ask for.
        BuildConfig {
            luxel_size: 1.0e-6,
            ..base
        },
        BuildConfig {
            luxel_size: 1.0e9,
            ..base
        },
        BuildConfig {
            ambient: Vec3::new(-5.0, NAN, f32::INFINITY),
            light_gain: 1.0e30,
            ..base
        },
        BuildConfig {
            fog_start: NAN,
            fog_end: NAN,
            ..base
        },
        BuildConfig {
            fog_start: -5.0,
            fog_end: 1.0e30,
            ..base
        },
        BuildConfig { fov_y: NAN, ..base },
        BuildConfig { fov_y: 3.2, ..base },
    ];
    for cfg in configs {
        through_the_gpu(&device, &room(), &cfg, &[plain_view()]);
    }
}

#[test]
fn hostile_views_frame_sizes_actors_and_poses_draw() {
    let Some(device) = gpu::gpu() else { return };
    let level = LevelRenderer::from_level(&room(), materials(), &quick()).expect("builds");
    let views = [
        plain_view(),
        View::from_angles(eye(), NAN, 0.0, 0.0),
        View::from_angles(eye(), 1.0e30, 1.0e30, 1.0e30),
        View::from_angles(Vec3::new(1.0e30, 0.0, 0.0), 0.0, 0.0, 0.0),
        View::from_angles(Vec3::new(-5.0, -5.0, -5.0), 0.0, 0.0, 0.0),
        View {
            pos: eye(),
            orientation: Quat::from_axis_angle(Vec3::new(NAN, 0.0, 1.0), NAN),
        },
    ];
    let odd = |kind, pos, yaw| Actor { kind, pos, yaw };
    let actors = [
        odd(ActorKind::Enemy, Vec3::new(NAN, 0.0, 0.0), NAN),
        odd(ActorKind::Goal, Vec3::new(1.0e30, 1.0e30, 0.0), 1.0e30),
        odd(ActorKind::Enemy, eye() + Vec3::new(0.0, 1.0, 0.0), 0.0),
        odd(ActorKind::Goal, eye(), 0.0),
    ];
    let poses = [MoverPose {
        rot: Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), NAN),
        pivot: Vec3::new(f32::INFINITY, 0.0, 0.0),
        offset: Vec3::new(1.0e30, NAN, 0.0),
    }];
    let sizes = [(1, 1), (2, 1), (1, 3), (17, 5), (301, 3), (MAX_SIDE, 2)];
    for mut renderer in on_gpu(&device, &level) {
        for view in &views {
            for (w, h) in sizes {
                let mut fb = Framebuffer::new(w, h).expect("a framebuffer");
                draw_all(&mut renderer, &level, &mut fb, view, &actors, &poses);
            }
        }
    }
}

/// Many small solids in a room, each `material` of its index, and each a mover
/// of its own when `movers`.
fn crowded(count: u16, material: impl Fn(u16) -> u16, movers: bool) -> Level {
    let mut b = LevelBuilder::new();
    b.room([0.0, 0.0, 0.0], [40.0, 40.0, 8.0], (0, 1, 2));
    for i in 0..count {
        let (x, y) = (
            f64::from(i % 30) * 1.25 + 1.0,
            f64::from(i / 30) * 1.25 + 1.0,
        );
        b.solid([x, y, 0.5], [x + 0.5, y + 0.5, 1.0], material(i));
    }
    let mut level = Level {
        brushes: b.finish(),
        entities: vec![light()],
        ..Default::default()
    };
    if movers {
        let first = level.brushes.len() - usize::from(count);
        level.mover_groups = (first..level.brushes.len())
            .map(|brush| MoverGroup {
                name: format!("m{brush}"),
                tags: Vec::new(),
                brushes: vec![u32::try_from(brush).expect("few brushes")],
            })
            .collect();
    }
    level
}

#[test]
fn many_faces_materials_and_movers_build_and_draw() {
    let Some(device) = gpu::gpu() else { return };
    let cfg = BuildConfig {
        luxel_size: 2.0,
        ..quick()
    };
    let over = View::from_angles(Vec3::new(20.0, 2.0, 6.0), 1.57, -0.6, 0.0);
    // Every id up to the last, most of them unknown and so on the fallback.
    let spread = |i: u16| i.wrapping_mul(97) | if i.is_multiple_of(2) { 0xff00 } else { 0 };
    through_the_gpu(&device, &crowded(600, spread, false), &cfg, &[over]);
    through_the_gpu(&device, &crowded(900, |_| 0, true), &cfg, &[over]);
}

/// A wedge from `x = 2` to a knife edge at `x = 4`, a fiftieth of a unit at its
/// thick end: faces with a corner as sharp as a brush can make.
fn sliver() -> Vec<DPlane> {
    let across = Vec3d::new(0.01, -1.0, 0.0).normalized().expect("a normal");
    vec![
        DPlane::new(Vec3d::new(0.0, 1.0, 0.0), 5.0),
        DPlane::new(across, across.dot(Vec3d::new(4.0, 5.0, 0.0))),
        DPlane::new(Vec3d::new(-1.0, 0.0, 0.0), -2.0),
        DPlane::new(Vec3d::new(0.0, 0.0, 1.0), 2.0),
        DPlane::new(Vec3d::new(0.0, 0.0, -1.0), -1.0),
    ]
}

/// Brushes the validator lets through, at its edges: the thinnest the grid
/// allows, a sharp wedge, and a room far from the origin. The ones it refuses,
/// not finite or flat, are skipped by the builder as a level file's are.
#[test]
fn slivers_and_far_brushes_build_and_draw() {
    let Some(device) = gpu::gpu() else { return };
    let mut b = LevelBuilder::new();
    b.room([0.0, 0.0, 0.0], [10.0, 10.0, 6.0], (0, 1, 2));
    b.solid([6.0, 1.0, 0.0], [6.125, 1.125, 6.0], 3);
    b.solid([1.0, 8.0, 3.0], [9.0, 8.125, 3.125], 3);
    b.solid_planes(&sliver(), 3);
    b.solid([4.0, 4.0, 1.0], [f64::NAN, 5.0, 2.0], 3);
    b.solid([3.0, 3.0, 1.0], [3.0, 4.0, 2.0], 3);
    let far = 1.0e6;
    b.room([far, far, 0.0], [far + 10.0, far + 10.0, 6.0], (0, 1, 2));
    let level = Level {
        brushes: b.finish(),
        entities: vec![light()],
        ..Default::default()
    };
    let far_view = View::from_angles(Vec3::new(1.0e6 + 2.0, 1.0e6 + 2.0, 2.0), 0.7, 0.0, 0.0);
    let at_wedge = View::from_angles(Vec3::new(1.0, 6.0, 1.5), -0.6, 0.0, 0.0);
    through_the_gpu(
        &device,
        &level,
        &quick(),
        &[plain_view(), at_wedge, far_view],
    );
}

/// A dense field at the finest cells: chunks with as many triangles as a chunk
/// can hold.
fn foam(p: Vec3d) -> f64 {
    (p.x * 3.1).sin() * (p.y * 2.7).cos() + (p.z * 3.3).sin() * (p.x * 2.9).cos()
}

#[test]
fn dense_voxel_chunks_build_and_draw() {
    let Some(device) = gpu::gpu() else { return };
    let Ok(mut volume) = Volume::new(Op::Subtract, 0, CellSize::Quarter, [0; 3], [3, 3, 2]) else {
        panic!("a volume");
    };
    let mut ctx = Context::new(&volume.clone(), &[]);
    fill(&mut volume, &mut ctx, foam, 7);
    let level = Level {
        volumes: vec![volume],
        entities: vec![light()],
        ..Level::default()
    };
    let inside = View::from_angles(Vec3::new(4.0, 4.0, 2.0), 0.4, 0.1, 0.0);
    let outside = View::from_angles(Vec3::new(-20.0, -20.0, 10.0), 0.78, -0.3, 0.0);
    through_the_gpu(&device, &level, &quick(), &[inside, outside]);
}

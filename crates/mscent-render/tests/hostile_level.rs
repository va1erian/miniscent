//! Hostile levels, configs and views must come back as errors or odd
//! pictures, never as panics (debug builds also trap arithmetic overflow).

use std::path::Path;

use mscent_math::{Quat, Vec3};
use mscent_render::{
    Actor, ActorKind, BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Overlay, Quality,
    View,
};
use mscent_world::{Entity, Level, LevelBuilder};

fn materials() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("textures")
}

fn room() -> Level {
    let mut b = LevelBuilder::new();
    b.room([0.0, 0.0, 0.0], [10.0, 10.0, 6.0], (0, 1, 2));
    Level {
        brushes: b.finish(),
        entities: vec![Entity::Light {
            pos: Vec3::new(5.0, 5.0, 4.0),
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 12.0,
            intensity: 1.0,
        }],
        volumes: Vec::new(),
    }
}

fn draw_all(r: &LevelRenderer, view: &View, w: u32, h: u32, actors: &[Actor]) {
    let mut fb = Framebuffer::new(w, h).expect("fb");
    let hud = Overlay {
        health: i32::MAX,
        max_health: i32::MAX,
        fps: f32::INFINITY,
    };
    for q in [Quality::Low, Quality::Medium, Quality::High] {
        let _ = r.render(&mut fb, view, q, actors, Some(&hud));
    }
}

fn eye() -> Vec3 {
    Vec3::new(2.0, 2.0, 2.0)
}

#[test]
fn empty_and_light_free_levels_build_and_render() {
    for level in [
        Level::default(),
        Level {
            entities: vec![],
            ..room()
        },
    ] {
        if let Ok(r) = LevelRenderer::from_level(&level, materials(), &BuildConfig::default()) {
            draw_all(&r, &View::from_angles(eye(), 0.3, 0.0, 0.0), 64, 48, &[]);
        }
    }
}

#[test]
fn hostile_lights_are_errors_not_panics() {
    let nan = f32::NAN;
    let bad = [
        (Vec3::new(nan, 0.0, 0.0), 5.0, 1.0),
        (eye(), 0.0, 1.0),
        (eye(), -3.0, 1.0),
        (eye(), f32::INFINITY, 1.0),
        (eye(), 5.0, f32::MAX),
        (eye(), 5.0, nan),
        (Vec3::new(1.0e30, 0.0, 0.0), 5.0, 1.0),
        (eye(), 1.0e30, 1.0),
    ];
    for (pos, radius, intensity) in bad {
        let mut level = room();
        level.entities = vec![Entity::Light {
            pos,
            color: Vec3::new(1.0, 1.0, 1.0),
            radius,
            intensity,
        }];
        if let Ok(r) = LevelRenderer::from_level(&level, materials(), &BuildConfig::default()) {
            draw_all(&r, &View::from_angles(eye(), 0.3, 0.0, 0.0), 64, 48, &[]);
        }
    }
}

#[test]
fn hostile_build_configs_do_not_panic() {
    let nan = f32::NAN;
    let base = BuildConfig::default();
    let configs = [
        BuildConfig {
            luxel_size: 0.0,
            ..base
        },
        BuildConfig {
            luxel_size: nan,
            ..base
        },
        BuildConfig {
            luxel_size: -1.0,
            ..base
        },
        BuildConfig {
            luxel_size: 1.0e-6,
            ..base
        },
        BuildConfig {
            luxel_size: 1.0e9,
            ..base
        },
        BuildConfig {
            ambient: Vec3::new(-5.0, nan, f32::INFINITY),
            ..base
        },
        BuildConfig {
            light_gain: nan,
            ..base
        },
        BuildConfig {
            light_gain: -1.0,
            ..base
        },
        BuildConfig {
            light_gain: 1.0e30,
            ..base
        },
        BuildConfig {
            fog_start: nan,
            fog_end: nan,
            ..base
        },
        BuildConfig {
            fog_start: 5.0,
            fog_end: 1.0,
            ..base
        },
        BuildConfig {
            fog_start: -5.0,
            fog_end: 1.0e30,
            ..base
        },
        BuildConfig { fov_y: 0.0, ..base },
        BuildConfig { fov_y: nan, ..base },
        BuildConfig { fov_y: 3.2, ..base },
        BuildConfig {
            fov_y: -1.0,
            ..base
        },
    ];
    for cfg in configs {
        if let Ok(r) = LevelRenderer::from_level(&room(), materials(), &cfg) {
            draw_all(&r, &View::from_angles(eye(), 0.3, 0.0, 0.0), 64, 48, &[]);
        }
    }
}

#[test]
fn hostile_views_sizes_and_actors_do_not_panic() {
    let r = LevelRenderer::from_level(&room(), materials(), &BuildConfig::default()).unwrap();
    let nan = f32::NAN;
    let views = [
        View::from_angles(eye(), nan, 0.0, 0.0),
        View::from_angles(eye(), 1.0e30, 1.0e30, 1.0e30),
        View::from_angles(Vec3::new(1.0e30, 0.0, 0.0), 0.0, 0.0, 0.0),
        View::from_angles(Vec3::new(-5.0, -5.0, -5.0), 0.0, 0.0, 0.0),
        View::from_angles(Vec3::new(0.0, 0.0, 0.0), 0.0, 0.0, 0.0),
        View {
            pos: eye(),
            orientation: Quat::from_axis_angle(Vec3::ZERO, 1.0),
        },
        View {
            pos: eye(),
            orientation: Quat::from_axis_angle(Vec3::new(nan, 0.0, 1.0), nan),
        },
    ];
    let actors = [
        Actor {
            kind: ActorKind::Enemy,
            pos: Vec3::new(nan, 0.0, 0.0),
            yaw: nan,
        },
        Actor {
            kind: ActorKind::Goal,
            pos: Vec3::new(1.0e30, 1.0e30, 0.0),
            yaw: 1.0e30,
        },
        Actor {
            kind: ActorKind::Enemy,
            pos: eye() + Vec3::new(0.0, 1.0, 0.0),
            yaw: 0.0,
        },
        Actor {
            kind: ActorKind::Goal,
            pos: eye(),
            yaw: 0.0,
        },
    ];
    for v in &views {
        for (w, h) in [(1, 1), (2, 1), (1, 3), (17, 5), (64, 48), (301, 3)] {
            draw_all(&r, v, w, h, &actors);
        }
    }
}

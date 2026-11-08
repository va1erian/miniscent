//! Enemies drawn as their kinds' models in software frames (M19.3): where an
//! enemy stands, its design's pixels, and each kind its own picture.

use std::path::Path;

use mscent_math::Vec3;
use mscent_render::{
    BuildConfig, BuildError, Effects, FrameInput, Framebuffer, LevelRenderer, MaterialTable,
    Quality, View,
};
use mscent_world::kinds::stock_kinds;
use mscent_world::{Entity, Kind, Level, LevelBuilder, model};

const W: u32 = 160;
const H: u32 = 90;

/// A lit room with one enemy of `kind` ahead of the eye, playing `kinds`.
fn level(kinds: Vec<Kind>, kind: u8) -> Level {
    let mut b = LevelBuilder::new();
    b.room([0.0, 0.0, 0.0], [14.0, 10.0, 6.0], (0, 1, 2));
    Level {
        brushes: b.finish(),
        entities: vec![
            Entity::Light {
                pos: Vec3::new(6.0, 5.0, 5.0),
                color: Vec3::new(1.0, 1.0, 1.0),
                radius: 14.0,
                intensity: 1.0,
            },
            Entity::Enemy {
                pos: Vec3::new(7.0, 5.0, 3.0),
                kind,
            },
        ],
        kinds,
        ..Default::default()
    }
}

fn build(level: &Level) -> Result<LevelRenderer, BuildError> {
    let cfg = BuildConfig {
        luxel_size: 4.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    LevelRenderer::from_level(level, materials, &cfg)
}

/// The frame from the room's west end, looking east at the enemy.
fn frame(r: &LevelRenderer, models: bool) -> Framebuffer {
    let view = View::from_angles(
        Vec3::new(1.0, 5.0, 3.0),
        -core::f32::consts::FRAC_PI_2,
        0.0,
        0.0,
    );
    let mut fb = Framebuffer::new(W, H).expect("a frame");
    let input = FrameInput {
        view: &view,
        quality: Quality::Low,
        actors: r.actors(),
        effects: Effects {
            models: if models {
                r.assets().spawn_models()
            } else {
                None
            },
            ..Effects::default()
        },
        wants_depth: false,
    };
    r.draw_frame(&mut fb, &input).expect("drawn");
    fb
}

/// The pixels where two frames differ, as positions.
fn changed(a: &Framebuffer, b: &Framebuffer) -> Vec<(u32, u32)> {
    let pairs = a.pixels().iter().zip(b.pixels()).enumerate();
    let differ = pairs.filter(|(_, (p, q))| p != q);
    differ.map(|(i, _)| (i as u32 % W, i as u32 / W)).collect()
}

#[test]
fn an_enemy_is_its_kinds_model_where_it_stands() {
    let r = build(&level(Vec::new(), 0)).expect("builds");
    assert!(r.actors().is_empty(), "no enemy is an actor any more");
    let changed = changed(&frame(&r, false), &frame(&r, true));
    assert!(changed.len() > 150, "{} pixels", changed.len());
    // Six units ahead on the axis of the view: the middle of the frame.
    let (sx, sy) = changed
        .iter()
        .fold((0, 0), |(x, y), &(a, b)| (x + a, y + b));
    let n = changed.len() as u32;
    let (cx, cy) = (sx / n, sy / n);
    assert!(
        cx.abs_diff(W / 2) < 12 && cy.abs_diff(H / 2) < 12,
        "at ({cx}, {cy})"
    );
}

#[test]
fn each_kind_draws_its_own_design() {
    let sentry = frame(&build(&level(Vec::new(), 0)).expect("builds"), true);
    let dart = frame(&build(&level(Vec::new(), 1)).expect("builds"), true);
    let mut kinds = stock_kinds();
    let mantis = include_str!("../../../assets/enemies/mantis.msb");
    kinds[0].blueprint = model::read(mantis.as_bytes()).expect("reads");
    let own = frame(&build(&level(kinds, 0)).expect("builds"), true);
    assert!(changed(&sentry, &dart).len() > 100);
    assert!(changed(&sentry, &own).len() > 100);
}

#[test]
fn a_kind_that_will_not_build_fails_the_level() {
    let head = "miniscent-blueprint 1\nname heavy\nclass drone\nskin hull metal\n";
    let smooths = "smooth sphere 0 0 0 12\n".repeat(model::MAX_PIECES - 1);
    let text = format!("{head}part p cell=0.25\nadd sphere 0 0 0 8 hull\n{smooths}");
    let mut kinds = stock_kinds();
    kinds[1].blueprint = model::read(text.as_bytes()).expect("reads");
    // Built in code, so no reader has judged it: the build refuses it.
    assert_eq!(build(&level(kinds, 0)).err(), Some(BuildError::Model(1)));
}

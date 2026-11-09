//! Voxels in a vista layer, judged by reading real pixels: a mountain range in a
//! layer's room draws through the hole, in front of it nothing changes, other
//! rooms' voxels never show, and the layer's eye follows the player as scale says.

use std::path::Path;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Quality, View};
use mscent_world::materials::{METAL, NO_DRAW, ROCK, VIEW_FIRST};
use mscent_world::vista_scale::parallax_of_scale;
use mscent_world::voxel::{CellSize, MountainRange, Volume, mountains};
use mscent_world::{Brush, Entity, Level, MAX_VISTA_FOG, Op};

const W: u32 = 160;
const H: u32 = 90;
const SKY: u32 = 0xff12_3456;
/// Where the layer's room is, and another room that no layer shows.
const AWAY: f64 = 1000.0;
const ELSEWHERE: f64 = 3000.0;

fn materials() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("textures")
}

fn vd(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn cuboid(min: Vec3d, max: Vec3d, op: Op, order: u32, material: u16) -> Brush {
    Brush::cuboid(min, max, op, order)
        .expect("a cuboid")
        .with_material(material)
}

fn config() -> BuildConfig {
    BuildConfig {
        luxel_size: 8.0,
        ambient: Vec3::new(0.4, 0.4, 0.45),
        shadow_samples: 1,
        sky_top: SKY,
        sky_bottom: SKY,
        fog_start: 500.0,
        fog_end: 1000.0,
        ..BuildConfig::default()
    }
}

fn layer(parallax: f32) -> Entity {
    Entity::VistaLayer {
        pos: Vec3::new(AWAY as f32, 0.0, 0.0),
        centre: Vec3::ZERO,
        slot: 0,
        layer: 0,
        parallax,
        spin: Vec3::ZERO,
        pan: [0.0, 0.0],
        fog_start: 0.0,
        fog_end: MAX_VISTA_FOG,
        fog_color: 0,
    }
}

fn lamp(x: f64) -> Entity {
    Entity::Light {
        pos: Vec3::new(x as f32, 0.0, 30.0),
        color: Vec3::new(1.0, 1.0, 1.0),
        radius: 400.0,
        intensity: 1.2,
    }
}

/// A range 96 across, ahead of an eye at the room's middle.
fn range_at(x: f64, order: u32, seed: u32) -> Volume {
    let range = MountainRange {
        min: vd(x - 48.0, 24.0, -19.0),
        max: vd(x + 48.0, 120.0, 20.0),
        cell: CellSize::One,
        peak: 34.0,
        seed,
        material: 4,
        order,
        clear: None,
        edge: None,
    };
    mountains(&range).expect("a range")
}

struct Scene {
    /// Whether the play room's walls are viewports.
    hole: bool,
    /// Whether the layer's room holds a range.
    range: bool,
    /// Whether another room, which no layer shows, holds one.
    stray: bool,
    /// Whether a lamp lights the layer's room. Without one the light is flat, so
    /// two scales, which bake at different luxel sizes, still match pixel for pixel.
    lit: bool,
    parallax: f32,
}

impl Scene {
    fn new() -> Self {
        Self {
            hole: true,
            range: true,
            stray: false,
            lit: true,
            parallax: 0.0,
        }
    }

    fn level(&self, pillar: bool) -> Level {
        let wall = if self.hole { VIEW_FIRST } else { METAL };
        // The play room is wide, so the player can stand 64 units off its middle.
        let play = cuboid(
            vd(-100.0, -16.0, -16.0),
            vd(100.0, 48.0, 16.0),
            Op::Subtract,
            0,
            wall,
        );
        let (lo, hi) = (
            vd(AWAY - 128.0, -128.0, -20.0),
            vd(AWAY + 128.0, 128.0, 40.0),
        );
        let floor_top = vd(AWAY + 128.0, 128.0, -19.0);
        let other = (
            vd(ELSEWHERE - 64.0, -64.0, -20.0),
            vd(ELSEWHERE + 64.0, 64.0, 40.0),
        );
        let mut brushes = vec![
            play,
            cuboid(lo, hi, Op::Subtract, 1, METAL),
            cuboid(lo, floor_top, Op::Add, 2, ROCK),
            cuboid(other.0, other.1, Op::Subtract, 5, METAL),
        ];
        if pillar {
            brushes.push(cuboid(
                vd(2.0, 20.0, -16.0),
                vd(6.0, 24.0, 16.0),
                Op::Add,
                6,
                METAL,
            ));
        }
        let mut volumes = Vec::new();
        if self.range {
            volumes.push(range_at(AWAY, 3, 7));
        }
        if self.stray {
            volumes.push(range_at(ELSEWHERE, 4, 9));
        }
        Level {
            brushes,
            volumes,
            entities: [layer(self.parallax)]
                .into_iter()
                .chain(self.lit.then(|| lamp(AWAY)))
                .collect(),
            ..Level::default()
        }
    }

    fn renderer(&self) -> LevelRenderer {
        build(&self.level(false))
    }
}

fn build(level: &Level) -> LevelRenderer {
    LevelRenderer::from_level(level, materials(), &config()).expect("builds")
}

fn picture_at(r: &LevelRenderer, at: Vec3, quality: Quality) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H).expect("fb");
    let view = View::from_angles(at, 0.0, 0.0, 0.0);
    r.render(&mut fb, &view, quality, &[], None)
        .expect("renders");
    fb
}

fn picture(r: &LevelRenderer, at: Vec3) -> Framebuffer {
    picture_at(r, at, Quality::Low)
}

/// Pixels where two pictures differ.
fn differing(a: &Framebuffer, b: &Framebuffer) -> Vec<usize> {
    let pairs = a.pixels().iter().zip(b.pixels());
    (pairs.enumerate())
        .filter(|(_, (p, q))| p != q)
        .map(|(i, _)| i)
        .collect()
}

/// How far two pictures are from each other: the summed difference of every colour
/// channel. A count of differing pixels says too little here: on a textured rock,
/// any sub-pixel shift changes most of the texels it touches, so a small shift and a
/// large one count alike, while the size of the change still follows the shift.
fn moved(a: &Framebuffer, b: &Framebuffer) -> u64 {
    let channel = |p: u32, shift: u32| i64::from((p >> shift) & 0xff);
    let pairs = a.pixels().iter().zip(b.pixels());
    pairs
        .map(|(&p, &q)| {
            [0, 8, 16]
                .into_iter()
                .map(|s| channel(p, s).abs_diff(channel(q, s)))
                .sum::<u64>()
        })
        .sum()
}

#[test]
fn a_range_in_a_layer_draws_and_a_brush_in_front_of_the_hole_keeps_its_pixels() {
    let at = Vec3::ZERO;
    let with = |pillar, range| {
        let scene = Scene {
            range,
            ..Scene::new()
        };
        picture(&build(&scene.level(pillar)), at)
    };
    let changed = differing(&with(false, false), &with(false, true));
    assert!(
        changed.len() > 400,
        "the range covers {} pixels",
        changed.len()
    );
    // The pillar's pixels: where standing it up changes a picture with no layer
    // to show, a hole of plain gradient.
    let bare = |pillar| {
        let mut level = Scene::new().level(pillar);
        level
            .entities
            .retain(|e| !matches!(e, Entity::VistaLayer { .. }));
        picture(&build(&level), at)
    };
    let silhouette = differing(&bare(false), &bare(true));
    assert!(silhouette.len() > 100, "the pillar is on screen");
    let (a, b) = (with(true, false), with(true, true));
    for &i in &silhouette {
        assert_eq!(
            a.pixels()[i],
            b.pixels()[i],
            "pixel {i} of the pillar changed"
        );
    }
    // The range still shows round it.
    let outside = differing(&a, &b)
        .into_iter()
        .filter(|i| !silhouette.contains(i));
    assert!(outside.count() > 300);
}

#[test]
fn another_rooms_voxels_never_show_in_a_layer() {
    let plain = Scene {
        stray: false,
        ..Scene::new()
    };
    let strayed = Scene {
        stray: true,
        ..Scene::new()
    };
    let (a, b) = (plain.renderer(), strayed.renderer());
    for at in [
        Vec3::ZERO,
        Vec3::new(30.0, 4.0, 3.0),
        Vec3::new(-50.0, 10.0, -5.0),
    ] {
        assert_eq!(picture(&a, at).pixels(), picture(&b, at).pixels());
    }
}

#[test]
fn a_layers_voxels_show_in_the_main_scene_only_when_the_eye_is_in_their_room() {
    // No hole: the play room is plain metal, so nothing of the layer can show.
    let walled = |range| {
        Scene {
            hole: false,
            range,
            ..Scene::new()
        }
        .renderer()
    };
    let (empty, ranged) = (walled(false), walled(true));
    let here = Vec3::new(3.0, 2.0, 1.0);
    assert_eq!(
        picture(&empty, here).pixels(),
        picture(&ranged, here).pixels()
    );
    // From inside the layer's room the range is plain geometry.
    let inside = Vec3::new(AWAY as f32, 0.0, 0.0);
    let (p, q) = (picture(&empty, inside), picture(&ranged, inside));
    assert!(
        differing(&p, &q).len() > 400,
        "the range shows from inside its room"
    );
}

#[test]
fn every_worker_count_draws_the_same_layer_voxels_at_high_quality() {
    let mut r = Scene {
        parallax: 0.25,
        ..Scene::new()
    }
    .renderer();
    let at = Vec3::new(4.0, 1.0, 1.0);
    r.set_tiling(90, 1);
    let one = picture_at(&r, at, Quality::High);
    r.set_tiling(7, 6);
    let many = picture_at(&r, at, Quality::High);
    assert_eq!(one.pixels(), many.pixels());
    let depth = |fb: &Framebuffer| -> Vec<u32> {
        (0..H as i32)
            .flat_map(|y| (0..W as i32).map(move |x| (x, y)))
            .map(|(x, y)| fb.depth_at(x, y).unwrap_or(-1.0).to_bits())
            .collect()
    };
    assert_eq!(depth(&one), depth(&many));
}

#[test]
fn a_mountain_shifts_by_scale_as_the_layers_eye_follows_the_player() {
    let scale = parallax_of_scale(16.0);
    let flat = |parallax| {
        Scene {
            parallax,
            lit: false,
            ..Scene::new()
        }
        .renderer()
    };
    let (one, sixteen) = (flat(1.0), flat(scale));
    // The same layer eye, 4 units off its middle: 4 units of walking at scale 1,
    // 64 at scale 16. The pictures through the hole are the same.
    let near = picture(&one, Vec3::new(4.0, 0.0, 0.0));
    let far = picture(&sixteen, Vec3::new(64.0, 0.0, 0.0));
    assert_eq!(
        differing(&near, &far).len(),
        0,
        "the same eye sees the same"
    );
    // And 4 units at scale 16 moves the eye a sixteenth as far.
    let small = picture(&sixteen, Vec3::new(4.0, 0.0, 0.0));
    let quarter = picture(&one, Vec3::new(0.25, 0.0, 0.0));
    assert_eq!(
        differing(&small, &quarter).len(),
        0,
        "a sixteenth of the walk"
    );
    let still = picture(&sixteen, Vec3::ZERO);
    assert!(
        !differing(&still, &near).is_empty(),
        "walking moves the range"
    );
    // A textured rock texel that moves by any sub-pixel step changes, so the change
    // from the large walk saturates sooner than the small one's: the ratio is well
    // under the 16 of the walks, and 3 still tells a sixteenth from the whole.
    assert!(
        moved(&still, &small) * 3 < moved(&still, &near),
        "a sixteenth of the walk moves far less of the picture"
    );
}

#[test]
fn a_range_has_no_hole_to_the_gradient_from_above() {
    // A room of NO_DRAW faces, so the only thing to see is the range itself. Looking
    // down at it, a hole in its surface would show the gradient.
    let range = MountainRange {
        min: vd(AWAY - 96.0, -96.0, -19.0),
        max: vd(AWAY + 96.0, 96.0, 8.0),
        cell: CellSize::One,
        peak: 24.0,
        seed: 21,
        material: 4,
        order: 3,
        clear: None,
        edge: None,
    };
    let play = cuboid(
        vd(-100.0, -16.0, -16.0),
        vd(100.0, 48.0, 16.0),
        Op::Subtract,
        0,
        VIEW_FIRST,
    );
    let (lo, hi) = (
        vd(AWAY - 128.0, -128.0, -20.0),
        vd(AWAY + 128.0, 128.0, 60.0),
    );
    let level = Level {
        brushes: vec![play, cuboid(lo, hi, Op::Subtract, 1, NO_DRAW)],
        volumes: vec![mountains(&range).expect("a range")],
        entities: vec![layer(1.0)],
        ..Level::default()
    };
    let r = build(&level);
    let mut rng = mscent_math::rng::Rng::new(5);
    let mut fb = Framebuffer::new(W, H).expect("fb");
    for pose in 0..200 {
        let at = Vec3::new(
            rng.range_f64(-12.0, 12.0) as f32,
            rng.range_f64(-12.0, 12.0) as f32,
            rng.range_f64(12.0, 16.0) as f32,
        );
        let yaw = rng.range_f64(0.0, std::f64::consts::TAU) as f32;
        let pitch = -(rng.range_f64(1.25, 1.5) as f32);
        let view = View::from_angles(at, yaw, pitch, 0.0);
        r.render(&mut fb, &view, Quality::Low, &[], None)
            .expect("renders");
        let holes = fb.pixels().iter().filter(|&&c| c == SKY).count();
        assert_eq!(
            holes, 0,
            "pose {pose} at {at:?} shows {holes} gradient pixels"
        );
    }
}

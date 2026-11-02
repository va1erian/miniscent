//! Light and shadow in a layer's room, judged by reading pixels at High quality:
//! far geometry is lit by its own lamps, shadowed by brushes and voxels, reaches
//! ambient light, and shades with bump and highlights like the play space.
//!
//! The eye hangs above the room and looks straight down at a floor with a tower or
//! a hill on it and a lamp off to the west. A window of floor on the far side of the
//! tower is compared with the same window of a room without the tower.

use std::path::Path;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Quality, View};
use mscent_world::materials::{BRICK, METAL, ROCK, VIEW_FIRST};
use mscent_world::vista_scale::{luxel_factor, parallax_of_scale};
use mscent_world::voxel::{CellSize, Context, MountainRange, Volume, fill, mountains};
use mscent_world::{Brush, Entity, Level, MAX_VISTA_FOG, Op};

const W: u32 = 160;
const H: u32 = 90;
const AWAY: f64 = 1000.0;
/// Top of the brush floor, and of the voxel one.
const FLOOR: f64 = -19.0;
const VOXEL_FLOOR: f64 = -16.0;
/// How high the eye hangs.
const EYE_Z: f32 = 40.0;

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
        shadow_samples: 8,
        light_radius: 0.5,
        sky_top: 0xff12_3456,
        sky_bottom: 0xff12_3456,
        fog_start: 500.0,
        fog_end: 1000.0,
        ..BuildConfig::default()
    }
}

#[derive(Clone, Copy)]
struct Rig {
    /// A tower of metal on the floor, 12 across, or 2 for a pole.
    tower: bool,
    pole: bool,
    /// A hill of voxels on the floor.
    hill: bool,
    /// A floor of voxels, 3 above the brush floor, instead of the brush one.
    voxel_floor: bool,
    lamp: bool,
    parallax: f32,
    /// Height of the layer's eye.
    eye_z: f32,
    /// Material of the brush floor.
    skin: u16,
    /// How far the layer's eye is east of the room's middle.
    eye_x: f32,
}

impl Rig {
    fn new() -> Self {
        Self {
            tower: false,
            pole: false,
            hill: false,
            voxel_floor: false,
            lamp: true,
            parallax: 0.0,
            eye_z: EYE_Z,
            skin: ROCK,
            eye_x: 0.0,
        }
    }

    fn volumes(&self) -> Vec<Volume> {
        let mut out = Vec::new();
        if self.voxel_floor {
            let edge = 16.0 * CellSize::One.units();
            let side = 8.0 * edge;
            let origin = [(AWAY - side / 2.0) as i32, -side as i32 / 2, -24];
            let mut floor =
                Volume::new(Op::Add, 3, CellSize::One, origin, [8, 8, 2]).expect("floor");
            let mut ctx = Context::new(&floor, &[]);
            fill(&mut floor, &mut ctx, |p| VOXEL_FLOOR - p.z, 4);
            out.push(floor);
        }
        if self.hill {
            let range = MountainRange {
                min: vd(AWAY - 8.0, -8.0, -22.0),
                max: vd(AWAY + 8.0, 8.0, 21.0),
                cell: CellSize::One,
                peak: 38.0,
                seed: 5,
                material: 4,
                order: 4,
            };
            out.push(mountains(&range).expect("a hill"));
        }
        out
    }

    fn level(&self) -> Level {
        let play = cuboid(
            vd(-100.0, -16.0, -16.0),
            vd(100.0, 48.0, 16.0),
            Op::Subtract,
            0,
            VIEW_FIRST,
        );
        let (lo, hi) = (vd(AWAY - 64.0, -64.0, -20.0), vd(AWAY + 64.0, 64.0, 60.0));
        let mut brushes = vec![play, cuboid(lo, hi, Op::Subtract, 1, METAL)];
        if !self.voxel_floor {
            let top = vd(AWAY + 64.0, 64.0, FLOOR);
            brushes.push(cuboid(lo, top, Op::Add, 2, self.skin));
        }
        if self.tower || self.pole {
            let half = if self.pole { 1.0 } else { 6.0 };
            let (a, b) = (vd(AWAY - half, -half, -20.0), vd(AWAY + half, half, 10.0));
            brushes.push(cuboid(a, b, Op::Add, 6, METAL));
        }
        let layer = Entity::VistaLayer {
            pos: Vec3::new(AWAY as f32 + self.eye_x, 0.0, self.eye_z),
            centre: Vec3::ZERO,
            slot: 0,
            layer: 0,
            parallax: self.parallax,
            spin: Vec3::ZERO,
            pan: [0.0, 0.0],
            fog_start: 0.0,
            fog_end: MAX_VISTA_FOG,
            fog_color: 0,
        };
        let lamp = Entity::Light {
            pos: Vec3::new((AWAY - 30.0) as f32, 0.0, 30.0),
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 400.0,
            intensity: 1.0,
        };
        Level {
            brushes,
            volumes: self.volumes(),
            entities: [layer]
                .into_iter()
                .chain(self.lamp.then_some(lamp))
                .collect(),
            ..Level::default()
        }
    }

    fn renderer(&self, cfg: &BuildConfig) -> LevelRenderer {
        LevelRenderer::from_level(&self.level(), materials(), cfg).expect("builds")
    }
}

fn picture(r: &LevelRenderer, quality: Quality) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H).expect("fb");
    let down = -std::f32::consts::FRAC_PI_2;
    let view = View::from_angles(Vec3::ZERO, 0.0, down, 0.0);
    r.render(&mut fb, &view, quality, &[], None)
        .expect("renders");
    fb
}

/// Pixels per world unit at the floor, looking straight down from `EYE_Z`.
fn pixels_per_unit(floor: f64) -> f64 {
    let height = f64::from(EYE_Z) - floor;
    f64::from(H) / 2.0 / ((1.3f64 / 2.0).tan() * height)
}

/// Mean luminance of the floor between world offsets `x0..x1` and `y0..y1` of the
/// room's middle, in whichever direction screen-right is.
fn window(fb: &Framebuffer, floor: f64, x: (f64, f64), y: (f64, f64)) -> f64 {
    let s = pixels_per_unit(floor);
    let at = |dx: f64, dy: f64| {
        (
            (f64::from(W) / 2.0 + dx * s) as i32,
            (f64::from(H) / 2.0 - dy * s) as i32,
        )
    };
    let ((px0, py1), (px1, py0)) = (at(x.0, y.0), at(x.1, y.1));
    let (mut sum, mut n) = (0.0, 0.0);
    for py in py0.min(py1)..=py0.max(py1) {
        for px in px0.min(px1)..=px0.max(px1) {
            let c = fb.pixel(px, py).expect("on screen");
            sum += f64::from((c >> 16 & 255) + (c >> 8 & 255) + (c & 255)) / 3.0;
            n += 1.0;
        }
    }
    sum / n
}

/// The window behind the tower from the lamp, and its mirror in front.
fn shadow_side(fb: &Framebuffer, floor: f64) -> (f64, f64) {
    let (behind, front) = ((14.0, 26.0), (-26.0, -14.0));
    (
        window(fb, floor, behind, (-3.0, 3.0)),
        window(fb, floor, front, (-3.0, 3.0)),
    )
}

#[test]
fn a_tower_in_a_far_room_shadows_the_floor_at_every_scale() {
    let cfg = config();
    for parallax in [0.0, 1.0, parallax_of_scale(16.0), parallax_of_scale(64.0)] {
        let (bare, towered) = (
            Rig {
                parallax,
                ..Rig::new()
            },
            Rig {
                tower: true,
                parallax,
                ..Rig::new()
            },
        );
        let (a, b) = (
            picture(&bare.renderer(&cfg), Quality::High),
            picture(&towered.renderer(&cfg), Quality::High),
        );
        let ((lit_behind, lit_front), (dark_behind, dark_front)) =
            (shadow_side(&a, FLOOR), shadow_side(&b, FLOOR));
        let factor = luxel_factor(parallax);
        println!(
            "parallax {parallax} (luxel factor {factor}): behind {lit_behind:.1} -> {dark_behind:.1}, front {lit_front:.1} -> {dark_front:.1}"
        );
        assert!(lit_behind > 40.0, "the lamp lights the floor: {lit_behind}");
        assert!(
            dark_behind < lit_behind * 0.6,
            "the tower shadows its far side: {lit_behind} -> {dark_behind} at parallax {parallax}"
        );
        assert!(
            (dark_front - lit_front).abs() < lit_front * 0.1,
            "and not the lamp's side: {lit_front} -> {dark_front}"
        );
    }
}

#[test]
fn a_tower_shadows_a_voxel_floor_and_a_voxel_hill_shadows_a_brush_floor() {
    let cfg = config();
    let render = |rig: Rig| picture(&rig.renderer(&cfg), Quality::High);
    let voxel = Rig {
        voxel_floor: true,
        ..Rig::new()
    };
    let (bare, towered) = (
        render(voxel),
        render(Rig {
            tower: true,
            ..voxel
        }),
    );
    let (lit, _) = shadow_side(&bare, VOXEL_FLOOR);
    let (dark, _) = shadow_side(&towered, VOXEL_FLOOR);
    println!("tower on voxels: {lit:.1} -> {dark:.1}");
    assert!(lit > 40.0 && dark < lit * 0.6, "{lit} -> {dark}");

    let (flat, hilly) = (
        render(Rig::new()),
        render(Rig {
            hill: true,
            ..Rig::new()
        }),
    );
    let (lit, _) = shadow_side(&flat, FLOOR);
    let (dark, _) = shadow_side(&hilly, FLOOR);
    println!("hill on brush: {lit:.1} -> {dark:.1}");
    assert!(
        dark < lit * 0.75,
        "the hill shadows the floor behind it: {lit} -> {dark}"
    );
}

#[test]
fn ambient_light_and_the_soft_shadow_settings_reach_a_far_room() {
    // No lamp: ambient alone lifts the floor off black.
    let cfg = BuildConfig {
        ambient: Vec3::new(0.2, 0.2, 0.2),
        ..config()
    };
    let dark = Rig {
        lamp: false,
        ..Rig::new()
    };
    let (low, high) = (
        window(
            &picture(
                &dark.renderer(&BuildConfig {
                    ambient: Vec3::ZERO,
                    ..config()
                }),
                Quality::Low,
            ),
            FLOOR,
            (-10.0, 10.0),
            (-10.0, 10.0),
        ),
        window(
            &picture(&dark.renderer(&cfg), Quality::Low),
            FLOOR,
            (-10.0, 10.0),
            (-10.0, 10.0),
        ),
    );
    assert!(
        high > low + 10.0,
        "ambient reaches the room: {low} -> {high}"
    );
    // The lamp's disc and sample count shape the penumbra of a far shadow: a hard
    // bake and a soft one differ.
    let towered = Rig {
        tower: true,
        ..Rig::new()
    };
    let hard = BuildConfig {
        light_radius: 0.0,
        shadow_samples: 1,
        ..config()
    };
    let soft = BuildConfig {
        light_radius: 6.0,
        shadow_samples: 32,
        ..config()
    };
    let (a, b) = (
        picture(&towered.renderer(&hard), Quality::Low),
        picture(&towered.renderer(&soft), Quality::Low),
    );
    let moved = a
        .pixels()
        .iter()
        .zip(b.pixels())
        .filter(|(p, q)| p != q)
        .count();
    assert!(
        moved > 200,
        "soft shadows apply there: {moved} pixels differ"
    );
}

/// A shot from low over the floor, as a layer is seen from close by.
fn low_shot(rig: Rig, quality: Quality) -> Framebuffer {
    let r = rig.renderer(&config());
    let mut fb = Framebuffer::new(W, H).expect("fb");
    let view = View::from_angles(Vec3::ZERO, 0.3, -0.9, 0.0);
    r.render(&mut fb, &view, quality, &[], None)
        .expect("renders");
    fb
}

fn count(a: &Framebuffer, b: &Framebuffer) -> usize {
    (a.pixels().iter().zip(b.pixels()))
        .filter(|(p, q)| p != q)
        .count()
}

#[test]
fn high_quality_shades_a_far_lit_layer_with_bump_and_highlights() {
    let close = Rig {
        tower: true,
        hill: true,
        eye_z: -10.0,
        eye_x: -24.0,
        ..Rig::new()
    };
    // Bump needs a normal map, which brick has; highlights need a glossy surface.
    let brick = Rig {
        skin: BRICK,
        ..close
    };
    let (low, medium) = (
        low_shot(brick, Quality::Low),
        low_shot(brick, Quality::Medium),
    );
    assert!(
        count(&low, &medium) > 500,
        "bump shows in a layer: {}",
        count(&low, &medium)
    );
    let metal = Rig {
        skin: METAL,
        ..close
    };
    let (medium, high) = (
        low_shot(metal, Quality::Medium),
        low_shot(metal, Quality::High),
    );
    assert!(
        count(&medium, &high) > 20,
        "highlights show in a layer: {}",
        count(&medium, &high)
    );
}

#[test]
fn a_thin_poles_shadow_survives_the_coarse_luxels_of_a_far_room() {
    // A pole 2 units across casts a strip 2 units wide: one luxel at 2-unit
    // luxels would wash it out (it did, at four units), so the base factor is fine
    // enough to keep it at every scale a layer can have.
    let cfg = config();
    for parallax in [0.0, 1.0, parallax_of_scale(2.0), parallax_of_scale(16.0)] {
        let strip = |fb: &Framebuffer| window(fb, FLOOR, (16.0, 24.0), (-0.5, 0.5));
        let bare = picture(
            &Rig {
                parallax,
                ..Rig::new()
            }
            .renderer(&cfg),
            Quality::High,
        );
        let pole = picture(
            &Rig {
                pole: true,
                parallax,
                ..Rig::new()
            }
            .renderer(&cfg),
            Quality::High,
        );
        let (lit, dark) = (strip(&bare), strip(&pole));
        assert!(dark < lit * 0.5, "parallax {parallax}: {lit} -> {dark}");
    }
}

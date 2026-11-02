//! Viewport faces on screen, judged by reading real pixels. A room whose far wall
//! is a viewport, looking into a sealed room far away that holds one bright panel.

use std::f32::consts::FRAC_PI_2;
use std::path::Path;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Quality, View};
use mscent_world::materials::{LIGHTPANEL, METAL, ROCK, VIEW_FIRST};
use mscent_world::{Brush, Entity, Level, MAX_VISTA_FOG, Op};

const W: u32 = 160;
const H: u32 = 90;
const SKY: u32 = 0xff12_3456;
/// Where the vista's room is.
const AWAY: f64 = 1000.0;

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

/// A room centred on `x`, its far wall at y = 48.
fn room_at(x: f64, order: u32, material: u16) -> Brush {
    cuboid(
        vd(x - 16.0, -16.0, -16.0),
        vd(x + 16.0, 48.0, 16.0),
        Op::Subtract,
        order,
        material,
    )
}

fn layer(x: f32, slot: u8, layer: u8, parallax: f32) -> Entity {
    Entity::VistaLayer {
        pos: Vec3::new(x, 0.0, 0.0),
        centre: Vec3::ZERO,
        slot,
        layer,
        parallax,
        spin: Vec3::ZERO,
        pan: [0.0, 0.0],
        fog_start: 0.0,
        fog_end: MAX_VISTA_FOG,
        fog_color: 0,
    }
}

/// The play room: all viewport, and a panel far off in a sealed room of metal.
fn level(parallax: f32) -> Level {
    let far = AWAY as f32;
    Level {
        brushes: vec![
            room_at(0.0, 0, VIEW_FIRST),
            room_at(AWAY, 1, METAL),
            cuboid(
                vd(AWAY - 2.0, 30.0, -2.0),
                vd(AWAY + 2.0, 31.0, 2.0),
                Op::Add,
                2,
                LIGHTPANEL,
            ),
        ],
        entities: vec![layer(far, 0, 0, parallax)],
        ..Level::default()
    }
}

fn config() -> BuildConfig {
    BuildConfig {
        luxel_size: 8.0,
        shadow_samples: 1,
        sky_top: SKY,
        sky_bottom: SKY,
        fog_start: 500.0,
        fog_end: 1000.0,
        ..BuildConfig::default()
    }
}

fn renderer(level: &Level) -> LevelRenderer {
    LevelRenderer::from_level(level, materials(), &config()).expect("builds")
}

fn picture(r: &LevelRenderer, at: Vec3) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H).expect("fb");
    let view = View::from_angles(at, 0.0, 0.0, 0.0);
    r.render(&mut fb, &view, Quality::Low, &[], None)
        .expect("renders");
    fb
}

/// Centre of the panel's pixels: with no lamp, nothing else in the room is lit.
fn panel_centre(fb: &Framebuffer) -> Option<(f32, f32)> {
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let c = fb.pixel(x, y).unwrap_or(0);
            let lum = (c >> 16 & 255) + (c >> 8 & 255) + (c & 255);
            if lum > 120 {
                sx += x as f32;
                sy += y as f32;
                n += 1.0;
            }
        }
    }
    (n > 0.0).then(|| (sx / n, sy / n))
}

#[test]
fn a_viewport_wall_shows_the_room_behind_it() {
    let r = renderer(&level(0.0));
    let fb = picture(&r, Vec3::ZERO);
    let (x, y) = panel_centre(&fb).expect("the panel shows");
    assert!((x - 80.0).abs() < 2.0 && (y - 45.0).abs() < 2.0, "{x} {y}");
    assert!(fb.pixels().iter().all(|&c| c >> 24 == 255), "marks remain");
    assert!(
        fb.pixels().iter().all(|&c| c != SKY),
        "a closed room covers the whole hole"
    );
}

#[test]
fn parallax_zero_never_shifts_and_turning_still_moves_the_view() {
    let r = renderer(&level(0.0));
    let here = panel_centre(&picture(&r, Vec3::ZERO)).expect("shows");
    let there = panel_centre(&picture(&r, Vec3::new(5.0, 0.0, 3.0))).expect("shows");
    assert_eq!(here, there, "the eye moved, the view did not");
    let mut fb = Framebuffer::new(W, H).expect("fb");
    let turned = View::from_angles(Vec3::ZERO, 0.15, 0.0, 0.0);
    r.render(&mut fb, &turned, Quality::Low, &[], None)
        .expect("renders");
    let moved = panel_centre(&fb).expect("shows");
    assert!((moved.0 - here.0).abs() > 5.0, "{moved:?} {here:?}");
}

#[test]
fn parallax_scales_how_far_the_view_shifts() {
    let shift = |parallax| {
        let r = renderer(&level(parallax));
        let a = panel_centre(&picture(&r, Vec3::ZERO)).expect("shows");
        let b = panel_centre(&picture(&r, Vec3::new(8.0, 0.0, 0.0))).expect("shows");
        (b.0 - a.0).abs()
    };
    let (half, full) = (shift(0.5), shift(1.0));
    assert!(full > 4.0, "{full}");
    assert!((half * 2.0 - full).abs() < 2.0, "{half} of {full}");
}

#[test]
fn every_worker_count_draws_the_same_vista() {
    let mut r = renderer(&level(0.25));
    let at = Vec3::new(2.0, 1.0, 1.0);
    let one = {
        r.set_tiling(90, 1);
        picture(&r, at)
    };
    r.set_tiling(7, 4);
    let many = picture(&r, at);
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
fn a_hole_keeps_its_true_depth_and_a_brush_in_front_keeps_its_pixels() {
    let plain = renderer(&level(0.0));
    let fb = picture(&plain, Vec3::ZERO);
    let (cx, cy) = (W as i32 / 2, H as i32 / 2);
    let wall = 1.0 / 48.0;
    let depth = fb.depth_at(cx, cy).expect("in range");
    assert!((depth - wall).abs() < 1e-3, "main depth is kept: {depth}");

    let mut blocked = level(0.0);
    blocked.brushes.push(cuboid(
        vd(-3.0, 10.0, -3.0),
        vd(3.0, 11.0, 3.0),
        Op::Add,
        3,
        ROCK,
    ));
    let fb = picture(&renderer(&blocked), Vec3::ZERO);
    assert!(
        (fb.depth_at(cx, cy).expect("in range") - 0.1).abs() < 1e-2,
        "the brush is nearer"
    );
    assert!(panel_centre(&fb).is_none(), "and it hides the panel");
}

#[test]
fn a_slot_with_no_layer_shows_the_gradient() {
    let mut l = level(0.0);
    l.entities.clear();
    let fb = picture(&renderer(&l), Vec3::ZERO);
    assert!(fb.pixels().iter().all(|&c| c == SKY));
}

#[test]
fn an_eye_outside_its_island_sees_no_other_room() {
    // The centre carries the eye through the far room's +X wall, in solid, looking
    // on along +X. A room further that way shows if the layer falls back to every
    // leaf the way a frame does, and from here the far room's walls are all behind
    // the eye or facing away.
    let outside = |decoy: bool| {
        let mut l = level(1.0);
        if let Some(Entity::VistaLayer { centre, .. }) = l.entities.first_mut() {
            *centre = Vec3::new(-10.0, 0.0, 0.0);
        }
        if decoy {
            l.brushes.push(room_at(2.0 * AWAY, 3, ROCK));
        }
        let r = renderer(&l);
        let mut fb = Framebuffer::new(W, H).expect("fb");
        let view = View::from_angles(Vec3::new(14.0, 0.0, 0.0), -FRAC_PI_2, 0.0, 0.0);
        r.render(&mut fb, &view, Quality::Low, &[], None)
            .expect("renders");
        fb
    };
    let empty = outside(false);
    assert!(empty.pixels().iter().all(|&c| c == SKY), "nothing in view");
    assert_eq!(empty.pixels(), outside(true).pixels(), "the decoy showed");
}

#[test]
fn a_viewport_face_in_a_layer_shows_the_layer_behind() {
    let far = AWAY as f32;
    let near = 2.0 * far;
    let mut l = level(0.0);
    // The nearer layer is an empty room walled only by viewport faces.
    l.brushes.push(room_at(2.0 * AWAY, 3, VIEW_FIRST));
    l.entities.push(layer(near, 0, 1, 0.0));
    let fb = picture(&renderer(&l), Vec3::ZERO);
    assert!(panel_centre(&fb).is_some(), "layer 0 shows through layer 1");
}

#[test]
fn a_shot_on_a_viewport_face_finds_no_surface() {
    let r = renderer(&level(0.0));
    let wall = Vec3::new(0.0, 48.0, 0.0);
    let normal = Vec3::new(0.0, -1.0, 0.0);
    assert!(r.scene().surface_at(wall, normal).is_none());
    assert!(r.scene().faces().iter().all(|f| f.material != VIEW_FIRST));
}

#[test]
fn a_layer_with_no_viewport_face_changes_nothing() {
    let plain = |entities| Level {
        brushes: vec![room_at(0.0, 0, METAL), room_at(AWAY, 1, METAL)],
        entities,
        ..Level::default()
    };
    let a = renderer(&plain(Vec::new()));
    let b = renderer(&plain(vec![layer(AWAY as f32, 0, 0, 0.0)]));
    assert_eq!(
        picture(&a, Vec3::ZERO).pixels(),
        picture(&b, Vec3::ZERO).pixels()
    );
}

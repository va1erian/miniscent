//! A vista over time (spin, pan), its fog and its layers, judged by reading real
//! pixels. The play room is all viewport; the layer is a sealed room far away
//! with one bright panel in it, and walls of whatever material a test needs.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::path::Path;

use mscent_math::{Vec3, Vec3d};
use mscent_render::scene::SceneStats;
use mscent_render::{
    BuildConfig, Effects, Framebuffer, LevelRenderer, MaterialId, MaterialTable, Quality, View,
};
use mscent_world::materials::{LIGHTPANEL, METAL, ROCK, SKY_CLOUDS, SKY_STARS, VIEW_FIRST};
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

/// What a layer does and looks like, beyond where it is.
#[derive(Clone, Copy)]
struct Look {
    spin: Vec3,
    pan: [f32; 2],
    fog_start: f32,
    fog_end: f32,
    fog_color: u32,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            spin: Vec3::ZERO,
            pan: [0.0, 0.0],
            fog_start: 0.0,
            fog_end: MAX_VISTA_FOG,
            fog_color: 0,
        }
    }
}

fn layer(x: f32, order: u8, look: Look) -> Entity {
    Entity::VistaLayer {
        pos: Vec3::new(x, 0.0, 0.0),
        centre: Vec3::ZERO,
        slot: 0,
        layer: order,
        parallax: 0.0,
        spin: look.spin,
        pan: look.pan,
        fog_start: look.fog_start,
        fog_end: look.fog_end,
        fog_color: look.fog_color,
    }
}

fn panel(x: f64, order: u32) -> Brush {
    cuboid(
        vd(x - 2.0, 30.0, -2.0),
        vd(x + 2.0, 31.0, 2.0),
        Op::Add,
        order,
        LIGHTPANEL,
    )
}

/// An all-viewport play room and one layer: a sealed room of `walls` with a panel.
fn level(walls: u16, look: Look) -> Level {
    Level {
        brushes: vec![
            room_at(0.0, 0, VIEW_FIRST),
            room_at(AWAY, 1, walls),
            panel(AWAY, 2),
        ],
        entities: vec![layer(AWAY as f32, 0, look)],
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

fn frame_sized(
    r: &LevelRenderer,
    view: &View,
    time: f32,
    (w, h): (u32, u32),
) -> (Framebuffer, SceneStats) {
    let mut fb = Framebuffer::new(w, h).expect("fb");
    let effects = Effects {
        vista_time: time,
        ..Effects::default()
    };
    let stats = r
        .render_with_effects(&mut fb, view, Quality::Low, &[], None, &effects)
        .expect("renders");
    (fb, stats)
}

fn frame(r: &LevelRenderer, view: &View, time: f32) -> (Framebuffer, SceneStats) {
    frame_sized(r, view, time, (W, H))
}

fn ahead() -> View {
    View::from_angles(Vec3::ZERO, 0.0, 0.0, 0.0)
}

fn picture(r: &LevelRenderer, time: f32) -> Framebuffer {
    frame(r, &ahead(), time).0
}

fn depths(fb: &Framebuffer) -> Vec<u32> {
    let (w, h) = (fb.width() as i32, fb.height() as i32);
    let all = (0..h).flat_map(|y| (0..w).map(move |x| (x, y)));
    all.map(|(x, y)| fb.depth_at(x, y).unwrap_or(-1.0).to_bits())
        .collect()
}

fn luminance(c: u32) -> u32 {
    (c >> 16 & 255) + (c >> 8 & 255) + (c & 255)
}

/// Centre of the panel's pixels: with no lamp, nothing else in a metal room is lit.
fn panel_centre(fb: &Framebuffer) -> Option<(f32, f32)> {
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if luminance(fb.pixel(x, y).unwrap_or(0)) > 120 {
                sx += x as f32;
                sy += y as f32;
                n += 1.0;
            }
        }
    }
    (n > 0.0).then(|| (sx / n, sy / n))
}

fn spinning() -> Look {
    Look {
        spin: Vec3::new(0.0, 0.0, 0.5),
        pan: [37.0, 11.0],
        ..Look::default()
    }
}

#[test]
fn time_zero_draws_the_picture_of_a_layer_that_does_not_move() {
    let still = renderer(&level(SKY_CLOUDS, Look::default()));
    let moving = renderer(&level(SKY_CLOUDS, spinning()));
    let (a, b) = (picture(&still, 0.0), picture(&moving, 0.0));
    assert_eq!(a.pixels(), b.pixels());
    assert_eq!(depths(&a), depths(&b));
    // Not a time at all: the same as 0.
    for odd in [-2.5, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(a.pixels(), picture(&moving, odd).pixels(), "time {odd}");
    }
    assert_ne!(a.pixels(), picture(&moving, 1.0).pixels(), "time 1 moves");
}

#[test]
fn spin_about_z_turns_the_view_as_the_player_turning_would() {
    let rate = 0.5;
    let look = Look {
        spin: Vec3::new(0.0, 0.0, rate),
        ..Look::default()
    };
    let r = renderer(&level(METAL, look));
    let at_rest = renderer(&level(METAL, Look::default()));
    let start = panel_centre(&picture(&r, 0.0)).expect("shows");
    let mut last = start.0;
    for time in [0.1, 0.2, 0.3] {
        let x = panel_centre(&picture(&r, time)).expect("shows").0;
        assert!(x > last + 2.0, "keeps moving: {x} after {last}");
        last = x;
    }
    // The whole screen is hole, so a player who turns by the same angle sees the
    // same layer through the same window.
    let turned = View::from_angles(Vec3::ZERO, rate * 0.3, 0.0, 0.0);
    let player = panel_centre(&frame(&at_rest, &turned, 0.0).0).expect("shows");
    let spun = panel_centre(&picture(&r, 0.3)).expect("shows");
    assert!(
        (spun.0 - player.0).abs() < 1.5,
        "{spun:?} against {player:?}"
    );
    assert!(
        (spun.1 - player.1).abs() < 1.5,
        "{spun:?} against {player:?}"
    );
    assert!(
        (spun.0 - start.0).abs() > 5.0,
        "{spun:?} did not leave {start:?}"
    );
}

#[test]
fn a_full_revolution_brings_the_picture_back() {
    let look = Look {
        spin: Vec3::new(0.0, 0.0, FRAC_PI_2),
        ..Look::default()
    };
    let r = renderer(&level(METAL, look));
    let start = panel_centre(&picture(&r, 0.0)).expect("shows");
    assert!(panel_centre(&picture(&r, 1.0)).is_none(), "a quarter turn");
    for turns in [4.0, 400.0] {
        let again = panel_centre(&picture(&r, turns)).expect("shows");
        let off = (again.0 - start.0).abs() + (again.1 - start.1).abs();
        assert!(off < 0.1, "after {turns} s: {again:?} against {start:?}");
    }
}

#[test]
fn spin_leaves_the_main_scene_in_front_of_the_hole_where_it_is() {
    let look = Look {
        spin: Vec3::new(0.0, 0.0, 0.5),
        ..Look::default()
    };
    let mut l = level(METAL, look);
    l.brushes.push(cuboid(
        vd(-3.0, 10.0, -3.0),
        vd(3.0, 11.0, 3.0),
        Op::Add,
        3,
        ROCK,
    ));
    let r = renderer(&l);
    let (a, b) = (picture(&r, 0.0), picture(&r, 0.5));
    let inside = |fb: &Framebuffer| -> Vec<u32> {
        let rows = 36..54;
        let all = rows.flat_map(|y| (72..88).map(move |x| (x, y)));
        all.map(|(x, y)| fb.pixel(x, y).expect("in range"))
            .collect()
    };
    assert_eq!(inside(&a), inside(&b), "the block did not move");
    assert_ne!(a.pixels(), b.pixels(), "and the view round it did");
}

/// The sky room with a pan: its far wall faces the eye, seen straight on.
fn pan_level(pan: [f32; 2]) -> Level {
    let look = Look {
        pan,
        ..Look::default()
    };
    let mut l = level(SKY_CLOUDS, look);
    l.brushes.truncate(2);
    l
}

fn cloud_width() -> f32 {
    let table = materials();
    let m = table.get(MaterialId(SKY_CLOUDS)).expect("clouds");
    m.texture.width() as f32
}

#[test]
fn a_panned_sky_changes_with_time_and_repeats_after_a_whole_texture() {
    let w = cloud_width();
    let r = renderer(&pan_level([w, 0.0]));
    let start = picture(&r, 0.0);
    for time in [0.25, 0.5, 0.9] {
        assert_ne!(start.pixels(), picture(&r, time).pixels(), "time {time}");
    }
    // One texture per second: after 1 s, or 50 s, the same picture again.
    for time in [1.0, 50.0] {
        assert_eq!(start.pixels(), picture(&r, time).pixels(), "time {time}");
    }
    let v = renderer(&pan_level([0.0, w]));
    let start = picture(&v, 0.0);
    assert_ne!(start.pixels(), picture(&v, 0.5).pixels());
    assert_eq!(start.pixels(), picture(&v, 2.0).pixels());
}

/// The sum of luminance differences over a block of rows and columns.
fn mismatch(a: &Framebuffer, b: &Framebuffer, shift: i32, columns: (i32, i32)) -> u32 {
    let mut total = 0;
    for y in 80..100 {
        for x in columns.0..columns.1 {
            let (here, there) = (a.pixel(x - shift, y), b.pixel(x, y));
            total += luminance(here.expect("in range")).abs_diff(luminance(there.expect("in")));
        }
    }
    total
}

#[test]
fn pan_moves_the_picture_by_the_texels_it_names() {
    let size = (320, 180);
    let wall = 48.0_f32;
    let focal = size.1 as f32 * 0.5 / (config().fov_y * 0.5).tan();
    let r = renderer(&pan_level([0.0, 0.0]));
    // Texels along world +X on the far wall, which the eye sees straight on.
    let face = r.scene().faces().iter().find(|f| {
        let on_wall = f
            .polygon
            .vertices()
            .iter()
            .all(|v| (v.y - 48.0).abs() < 1e-6);
        on_wall && (f.polygon.vertices()[0].x - AWAY).abs() < 20.0
    });
    let info = face.expect("the far wall").texinfo;
    let origin = vd(AWAY, 48.0, 0.0);
    let (u0, v0) = info.texels_at(origin);
    let (u1, v1) = info.texels_at(origin + vd(1.0, 0.0, 0.0));
    let per_unit = (u1 - u0) as f32;
    assert!(
        per_unit.abs() > 0.1 && (v1 - v0).abs() < 1e-9,
        "u runs along x"
    );

    // The pan that should slide the picture 10 pixels at this distance.
    let texels = 10.0 * wall / focal * per_unit.abs();
    let moved = renderer(&pan_level([texels, 0.0]));
    let (before, _) = frame_sized(&moved, &ahead(), 0.0, size);
    let (after, _) = frame_sized(&moved, &ahead(), 1.0, size);
    let best = (-20..=20).min_by_key(|&s| mismatch(&before, &after, s, (140, 180)));
    let found = best.expect("a range") as f32;
    // Positive pan carries the picture towards +U.
    let expected = 10.0 * per_unit.signum();
    assert!(
        (found - expected).abs() <= 1.5,
        "moved {found}, wanted {expected}"
    );
}

#[test]
fn a_lit_face_in_a_panning_layer_does_not_change() {
    let look = Look {
        pan: [300.0, 200.0],
        ..Look::default()
    };
    let r = renderer(&level(METAL, look));
    let start = picture(&r, 0.0);
    assert!(panel_centre(&start).is_some());
    for time in [0.37, 1.0, 12.5] {
        assert_eq!(start.pixels(), picture(&r, time).pixels(), "time {time}");
    }
}

/// Luminance over a block of the picture.
fn block(fb: &Framebuffer, x: std::ops::Range<i32>, y: std::ops::Range<i32>) -> f32 {
    let all = y.flat_map(|y| x.clone().map(move |x| (x, y)));
    all.map(|(x, y)| luminance(fb.pixel(x, y).expect("in range")) as f32)
        .sum()
}

#[test]
fn layer_fog_darkens_with_distance_and_the_maximum_means_none() {
    let fogged = |end| Look {
        fog_start: 4.0,
        fog_end: end,
        ..Look::default()
    };
    let none = picture(&renderer(&level(SKY_STARS, Look::default())), 0.0);
    let by_max = picture(&renderer(&level(SKY_STARS, fogged(MAX_VISTA_FOG))), 0.0);
    assert_eq!(none.pixels(), by_max.pixels(), "the maximum is no fog");
    let fog = picture(&renderer(&level(SKY_STARS, fogged(90.0))), 0.0);
    // The far wall in the middle (48 away), and the floor under it (about 21).
    let ratio = |x: std::ops::Range<i32>, y: std::ops::Range<i32>| {
        block(&fog, x.clone(), y.clone()) / block(&none, x, y).max(1.0)
    };
    let (far, near) = (ratio(70..90, 38..52), ratio(70..90, 80..90));
    assert!(far < near && near < 0.95, "far {far}, near {near}");
    assert!(far < 0.7, "the far wall is mostly fog: {far}");
}

#[test]
fn layer_one_draws_over_layer_zero_in_either_file_order() {
    // Rock, not a sky: skies have cutouts, and layer 0 would show through them.
    let two = |reversed: bool| {
        let mut l = level(METAL, Look::default());
        l.brushes.push(room_at(2.0 * AWAY, 3, ROCK));
        let near = layer(2.0 * AWAY as f32, 1, Look::default());
        let far = layer(AWAY as f32, 0, Look::default());
        l.entities = if reversed {
            vec![near, far]
        } else {
            vec![far, near]
        };
        renderer(&l)
    };
    let (forward, reversed) = (picture(&two(false), 0.0), picture(&two(true), 0.0));
    assert_eq!(forward.pixels(), reversed.pixels());
    let under = renderer(&level(METAL, Look::default()));
    let layer_zero = picture(&under, 0.0);
    assert!(
        panel_centre(&layer_zero).is_some(),
        "layer 0 alone shows it"
    );
    assert_ne!(forward.pixels(), layer_zero.pixels(), "layer 1 covers it");
    // And it is the nearer layer's picture, by itself.
    let mut alone = level(METAL, Look::default());
    alone.brushes = vec![room_at(0.0, 0, VIEW_FIRST), room_at(2.0 * AWAY, 3, ROCK)];
    alone.entities = vec![layer(2.0 * AWAY as f32, 1, Look::default())];
    let only = picture(&renderer(&alone), 0.0);
    assert_eq!(forward.pixels(), only.pixels());
}

#[test]
fn every_worker_count_draws_the_same_moving_vista() {
    let look = Look {
        spin: Vec3::new(0.2, 0.1, 0.9),
        pan: [60.0, 25.0],
        fog_start: 4.0,
        fog_end: 120.0,
        ..Look::default()
    };
    let mut r = renderer(&level(SKY_CLOUDS, look));
    let view = View::from_angles(Vec3::new(2.0, 1.0, 1.0), 0.1, 0.05, 0.0);
    r.set_tiling(90, 1);
    let one = frame(&r, &view, 3.7).0;
    for (rows, workers) in [(7, 4), (24, 3)] {
        r.set_tiling(rows, workers);
        let many = frame(&r, &view, 3.7).0;
        assert_eq!(
            one.pixels(),
            many.pixels(),
            "{rows} rows, {workers} workers"
        );
        assert_eq!(depths(&one), depths(&many));
    }
    assert_ne!(one.pixels(), frame(&r, &view, 0.0).0.pixels(), "time shows");
}

/// A metal play room with a viewport window on its far wall, and a layer.
fn windowed(with_layer: bool) -> Level {
    let mut l = level(SKY_CLOUDS, Look::default());
    l.brushes[0] = room_at(0.0, 0, METAL);
    l.brushes.push(cuboid(
        vd(-6.0, 40.0, -6.0),
        vd(6.0, 48.0, 6.0),
        Op::Add,
        3,
        VIEW_FIRST,
    ));
    if !with_layer {
        l.entities.clear();
    }
    l
}

#[test]
fn vista_stats_count_what_a_vista_draws() {
    let r = renderer(&windowed(true));
    let seen = frame(&r, &ahead(), 0.0).1;
    assert!(seen.vista_pixels > 0 && seen.vista_faces > 0, "{seen:?}");
    assert!(
        seen.pixels > seen.vista_pixels,
        "main pixels are counted too"
    );

    let behind = View::from_angles(Vec3::ZERO, TAU / 2.0, 0.0, 0.0);
    let away = frame(&r, &behind, 0.0).1;
    assert_eq!((away.vista_pixels, away.vista_faces), (0, 0), "{away:?}");

    let plain = renderer(&windowed(false));
    let none = frame(&plain, &ahead(), 0.0).1;
    assert_eq!((none.vista_pixels, none.vista_faces), (0, 0), "{none:?}");
    let again = frame(&plain, &ahead(), 2.0).1;
    assert_eq!(none, again, "stats of a level with no layer do not change");
}

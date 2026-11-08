//! A projector's pool of light, read from real frames: a room lit by a projector
//! that shines straight down has a bright patch of floor under it and a dark floor
//! beyond the cone, where the same room lit by a point light of the same colour,
//! radius and strength is bright all the way out. Both renderers' pictures come
//! from the same bake, so one frame, by the software renderer, stands for both.

use std::path::Path;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Quality, View};
use mscent_world::{Brush, Entity, Level, Op};

const W: usize = 160;
const H: usize = 90;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A 60 by 60 room, 20 high, lit from its middle by `lamp`.
fn room(lamp: Entity) -> LevelRenderer {
    let b = Brush::cuboid(
        Vec3d::new(-30.0, -30.0, 0.0),
        Vec3d::new(30.0, 30.0, 20.0),
        Op::Subtract,
        0,
    )
    .expect("a brush");
    let level = Level {
        brushes: vec![b],
        entities: vec![lamp],
        ..Default::default()
    };
    let cfg = BuildConfig {
        luxel_size: 0.5,
        ambient: Vec3::ZERO,
        // Far fog would paint the floor beyond the pool the colour of the sky.
        fog_start: 900.0,
        fog_end: 1000.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    LevelRenderer::from_level(&level, materials, &cfg).expect("a level")
}

fn projector(outer: f32) -> Entity {
    Entity::Projector {
        pos: v(0.0, 0.0, 10.0),
        yaw: 0.0,
        pitch: -core::f32::consts::FRAC_PI_2,
        color: v(1.0, 1.0, 1.0),
        radius: 40.0,
        intensity: 2.0,
        inner: outer * 0.5,
        outer,
    }
}

fn point() -> Entity {
    Entity::Light {
        pos: v(0.0, 0.0, 10.0),
        color: v(1.0, 1.0, 1.0),
        radius: 40.0,
        intensity: 2.0,
    }
}

/// The frame as seen by an eye 9 above the floor's middle, looking straight down.
fn frame(r: &LevelRenderer) -> Vec<u32> {
    let mut fb = Framebuffer::new(W as u32, H as u32).expect("a target");
    let view = View::from_angles(v(0.0, 0.0, 19.0), 0.0, -core::f32::consts::FRAC_PI_2, 0.0);
    r.render(&mut fb, &view, Quality::Medium, &[], None)
        .expect("a frame");
    fb.pixels().to_vec()
}

fn brightness(p: u32) -> u32 {
    (p & 0xff) + ((p >> 8) & 0xff) + ((p >> 16) & 0xff)
}

/// Mean brightness of the block of pixels with corners `(x0, y0)` and `(x1, y1)`.
fn mean(frame: &[u32], (x0, y0): (usize, usize), (x1, y1): (usize, usize)) -> f64 {
    let mut sum = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            sum += u64::from(brightness(frame[y * W + x]));
        }
    }
    sum as f64 / ((x1 - x0) * (y1 - y0)) as f64
}

const CENTRE: ((usize, usize), (usize, usize)) = ((70, 40), (90, 50));
const CORNER: ((usize, usize), (usize, usize)) = ((0, 0), (20, 12));

#[test]
fn the_floor_under_a_projector_is_lit_and_the_floor_beyond_its_cone_is_not() {
    // The lamp is 10 up: a cone of 0.3 radians reaches 3 to the side.
    let shaft = frame(&room(projector(0.3)));
    let (under, away) = (
        mean(&shaft, CENTRE.0, CENTRE.1),
        mean(&shaft, CORNER.0, CORNER.1),
    );
    assert!(under > 3.0 * away, "under {under}, away {away}");
}

#[test]
fn a_point_light_of_the_same_strength_lights_the_corner_too() {
    let spread = frame(&room(point()));
    let shaft = frame(&room(projector(0.3)));
    let (omni, cone) = (
        mean(&spread, CORNER.0, CORNER.1),
        mean(&shaft, CORNER.0, CORNER.1),
    );
    assert!(omni > 1.5 * cone, "point light {omni}, projector {cone}");
}

#[test]
fn a_wider_cone_lights_more_of_the_floor() {
    let lit = |outer| {
        let f = frame(&room(projector(outer)));
        f.iter().filter(|&&p| brightness(p) > 120).count()
    };
    let (narrow, wide) = (lit(0.2), lit(0.6));
    assert!(wide > 2 * narrow, "{narrow} pixels against {wide}");
}

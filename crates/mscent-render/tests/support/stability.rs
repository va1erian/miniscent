//! Temporal stability metric shared by `examples/stability.rs` and
//! `tests/stability.rs`.
//!
//! The camera moves by a fraction of a pixel per frame (strafing, then
//! turning); a smooth image changes smoothly, so the second difference of one
//! pixel's luminance over three consecutive frames stays near zero except at
//! real geometry or shadow edges. Shimmer shows up as large second
//! differences on pixels in the interior of surfaces.
//!
//! Only the luminance of pixels on faces that stay under the pixel for the
//! whole sequence is compared: a pixel whose first and last frame differ by
//! more than `EDGE` levels is an edge that really crosses it and is skipped.

use std::path::Path;

use mscent_math::Vec3;
use mscent_render::{
    BuildConfig, Framebuffer, LevelRenderer, Material, MaterialTable, Quality, Texture, View,
};
use mscent_world::{Level, materials};

pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 360;
/// Frames per sequence.
pub const FRAMES: usize = 40;
/// Motion per frame, in pixels at the target.
pub const PX_PER_FRAME: f32 = 0.125;
/// A pixel that moves this many levels over the whole sequence crosses a
/// real edge and is excluded.
const EDGE: f32 = 40.0;
/// Second differences above this many levels count as shimmer.
pub const SHIMMER: f32 = 3.0;

/// How the level is textured for the measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// Every material a flat mid-grey, so only the lighting varies.
    Flat,
    /// The real textures with their normal maps.
    Real,
}

pub fn materials_for(look: Look) -> MaterialTable {
    match look {
        Look::Real => {
            let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
            MaterialTable::load_dir(&dir)
                .expect("load assets/textures")
                .filtered()
        }
        Look::Flat => {
            let mut t = MaterialTable::new();
            for name in materials::NAMES {
                let texture = Texture::solid(0xff80_8080).filtered();
                let name = name.to_string();
                t.add(Material {
                    name,
                    gloss: 0,
                    texture,
                });
            }
            t
        }
    }
}

/// Fog far away so distance does not fade the signal being measured.
pub fn config() -> BuildConfig {
    BuildConfig {
        fog_start: 500.0,
        fog_end: 600.0,
        ..BuildConfig::default()
    }
}

/// A measured camera path: where it starts, how it looks and how it moves.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub name: &'static str,
    pub eye: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Distance from the eye to the point being watched.
    pub dist: f32,
}

/// Three distances to the same shadowed patch of the start room's floor and
/// walls: the pillar at (11..13, 11..13) shadows the north east corner from
/// the lamp at (6, 6, 6.5).
pub fn poses() -> Vec<Pose> {
    let target = Vec3::new(15.5, 17.5, 0.0);
    let dir = (0.6f32, 0.8f32);
    [("near", 4.0f32), ("mid", 10.0), ("far", 18.0)]
        .into_iter()
        .map(|(name, d)| {
            let eye = Vec3::new(target.x - dir.0 * d, target.y - dir.1 * d, 2.5);
            Pose {
                name,
                eye,
                yaw: -dir.0.asin(),
                pitch: -(2.5f32).atan2(d) * 0.8,
                dist: d,
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Strafe,
    Turn,
}

fn view_at(p: &Pose, motion: Motion, step: usize) -> View {
    // World size of one pixel at the target distance.
    let px = p.dist * 2.0 * (0.5 * 1.3f32).tan() / HEIGHT as f32;
    let t = step as f32 * PX_PER_FRAME;
    match motion {
        Motion::Strafe => {
            // Sideways: perpendicular to the horizontal view direction.
            let right = Vec3::new(p.yaw.cos(), p.yaw.sin(), 0.0);
            View::from_angles(p.eye + right * (px * t), p.yaw, p.pitch, 0.0)
        }
        Motion::Turn => {
            let rad_per_px = 2.0 * (0.5 * 1.3f32).tan() / HEIGHT as f32;
            View::from_angles(p.eye, p.yaw - rad_per_px * t, p.pitch, 0.0)
        }
    }
}

fn luma(c: u32) -> f32 {
    let (r, g, b) = ((c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff);
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

/// Result for one pose and motion.
#[derive(Clone, Copy, Debug)]
pub struct Shimmer {
    /// Mean absolute second difference over the compared pixels, in levels.
    pub mean: f32,
    /// Percentage of compared pixels with a second difference above
    /// [`SHIMMER`].
    pub pct_over: f32,
    /// Largest second difference seen.
    pub max: f32,
    pub compared: usize,
    /// Mean render time per frame, in milliseconds.
    pub ms: f32,
}

pub fn measure(r: &LevelRenderer, pose: &Pose, motion: Motion, q: Quality) -> Shimmer {
    let mut fb = Framebuffer::new(WIDTH, HEIGHT).expect("framebuffer");
    let mut frames: Vec<Vec<f32>> = Vec::with_capacity(FRAMES);
    let mut render_time = std::time::Duration::ZERO;
    for step in 0..FRAMES {
        let view = view_at(pose, motion, step);
        let t = std::time::Instant::now();
        r.render(&mut fb, &view, q, &[], None).expect("render");
        render_time += t.elapsed();
        frames.push(fb.pixels().iter().map(|&c| luma(c)).collect());
    }
    let (mut sum, mut over, mut max, mut n) = (0.0f64, 0usize, 0.0f32, 0usize);
    for i in 0..frames[0].len() {
        // This pixel's luminance over time.
        let series: Vec<f32> = frames.iter().map(|f| f[i]).collect();
        let travel = series.windows(2).map(|w| (w[1] - w[0]).abs());
        let travel = travel.fold(0.0f32, f32::max);
        // Skip pixels that a real edge sweeps across.
        if (series[0] - series[FRAMES - 1]).abs() > EDGE || travel > EDGE * 0.5 {
            continue;
        }
        for w in series.windows(3) {
            let d2 = (w[2] - 2.0 * w[1] + w[0]).abs();
            sum += f64::from(d2);
            over += usize::from(d2 > SHIMMER);
            max = max.max(d2);
            n += 1;
        }
    }
    let n_f = n.max(1) as f32;
    Shimmer {
        mean: (sum / n.max(1) as f64) as f32,
        pct_over: 100.0 * over as f32 / n_f,
        max,
        compared: n,
        ms: render_time.as_secs_f32() * 1000.0 / FRAMES as f32,
    }
}

pub fn build(level: &Level, look: Look) -> LevelRenderer {
    LevelRenderer::from_level(level, materials_for(look), &config()).expect("level builds")
}

/// First frame of a pose as PNG bytes, 2x enlarged so shadow edges can be
/// inspected.
pub fn first_frame_png(r: &LevelRenderer, pose: &Pose, q: Quality) -> Vec<u8> {
    let mut fb = Framebuffer::new(WIDTH, HEIGHT).expect("framebuffer");
    r.render(&mut fb, &view_at(pose, Motion::Strafe, 0), q, &[], None)
        .expect("render");
    let (w, h) = (WIDTH as usize, HEIGHT as usize);
    let mut big = Vec::with_capacity(w * h * 4);
    for y in 0..h * 2 {
        for x in 0..w * 2 {
            big.push(fb.pixels()[(y / 2) * w + x / 2]);
        }
    }
    mscent_render::png_encode::encode_png(WIDTH * 2, HEIGHT * 2, &big).expect("encode")
}

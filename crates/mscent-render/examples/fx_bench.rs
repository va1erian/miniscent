//! Cost of the particle drawing kernels (M9.1): the table in
//! `docs/particles.md`, reproduced in the repo.
//!
//!   cargo run --release -p mscent-render --example fx_bench
//!
//! Everything draws into a 640x360 target on one thread. Each figure is the
//! fastest of several batches, which discards interference from other
//! processes. Sprite costs are per pixel of the sprite's box, so a figure is
//! the cost of one pixel whether or not the mask is empty there.

use std::hint::black_box;
use std::time::Instant;

use mscent_math::Vec3;
use mscent_math::rng::Rng;
use mscent_render::particles::{
    Mask, ParticleDraws, SpriteBank, SpriteDraw, StreakDraw, draw_particles_with,
};
use mscent_render::simd::Level;
use mscent_render::{Camera, Framebuffer, ScreenPoint, Texture, Vertex, draw_polygon_flat};

const W: u32 = 640;
const H: u32 = 360;
const BATCHES: usize = 9;
const TINT: u32 = 0xc040_8060;

/// Nanoseconds per call of `f`, the fastest of `BATCHES` batches of `reps`.
fn ns_per_call(reps: usize, mut f: impl FnMut()) -> f64 {
    f();
    (0..BATCHES)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..reps {
                f();
            }
            t.elapsed().as_nanos() as f64 / reps as f64
        })
        .fold(f64::MAX, f64::min)
}

fn sprite(x: f32, y: f32, radius: f32, inv_z: f32) -> SpriteDraw {
    SpriteDraw {
        x,
        y,
        radius,
        inv_z,
        tint: TINT,
        mask: Mask::Soft,
    }
}

/// A target with a wall at `1/z = 0.5` over the whole screen.
fn walled() -> Framebuffer {
    let cam = Camera::look_at(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 10.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        1.2,
        0.1,
        100.0,
        W,
        H,
    )
    .expect("camera");
    let tex = Texture::new(16, 16, vec![0xff80_8080; 256]).expect("texture");
    let v = |x: f32, z: f32| Vertex::new(Vec3::new(x, 2.0, z), 0.0, 0.0);
    let mut fb = Framebuffer::new(W, H).expect("target");
    let quad = [
        v(-50.0, -50.0),
        v(50.0, -50.0),
        v(50.0, 50.0),
        v(-50.0, 50.0),
    ];
    draw_polygon_flat(&mut fb, &cam, &quad, &tex, 256, None);
    assert!(
        (0..H as i32)
            .step_by(37)
            .all(|y| fb.depth_at(W as i32 / 2, y) == Some(0.5)),
        "the wall covers the screen"
    );
    fb
}

fn open() -> Framebuffer {
    Framebuffer::new(W, H).expect("target")
}

fn fill_table(bank: &SpriteBank) {
    println!("Sprite fill, ns per bbox pixel (lower is better)");
    println!("{:<34}{:>9}{:>9}{:>9}", "", "scalar", "SSE2", "AVX2");
    let cases = [
        ("add, 128 px (1:1 mip)", 64.0, false, false),
        ("add, 320 px (upscaled)", 160.0, false, false),
        ("over, 128 px", 64.0, true, false),
        ("over, 320 px", 160.0, true, false),
        ("add, 320 px, fully occluded", 160.0, false, true),
        ("over, 320 px, fully occluded", 160.0, true, true),
    ];
    for (name, radius, over, occluded) in cases {
        print!("{name:<34}");
        for level in Level::ALL {
            if !level.is_supported() {
                print!("{:>9}", "-");
                continue;
            }
            let mut fb = if occluded { walled() } else { open() };
            let s = sprite(
                W as f32 / 2.0,
                H as f32 / 2.0,
                radius,
                if occluded { 0.1 } else { 0.9 },
            );
            let mut draws = ParticleDraws::default();
            if over {
                draws.over.push(s);
            } else {
                draws.add.push(s);
            }
            let ns = ns_per_call(200, || {
                draw_particles_with(level, black_box(&mut fb), black_box(&draws), bank)
            });
            print!("{:>9.3}", ns / (4.0 * radius as f64 * radius as f64));
        }
        println!();
    }
}

fn streak_table(bank: &SpriteBank) {
    println!("\nSpark streak, width 1 / 3, ns per streak (ns per pixel of length)");
    let mut rng = Rng::new(7);
    for len in [4.0f32, 16.0, 64.0] {
        print!("{len:>4} px");
        for width in [1u8, 3] {
            let mut draws = ParticleDraws::default();
            for _ in 0..1000 {
                let (x, y) = (rng.range_f32(80.0, 560.0), rng.range_f32(80.0, 280.0));
                let a = rng.range_f32(0.0, std::f32::consts::TAU);
                let (dx, dy) = (a.cos() * len, a.sin() * len);
                let p = |x, y| ScreenPoint { x, y, inv_z: 0.5 };
                draws.streaks.push(StreakDraw {
                    a: p(x, y),
                    b: p(x + dx, y + dy),
                    width,
                    tint: 0xff80_a0ff,
                });
            }
            let mut fb = open();
            let ns = ns_per_call(100, || {
                draw_particles_with(Level::best(), black_box(&mut fb), black_box(&draws), bank)
            }) / 1000.0;
            print!("{:>12.1} ({:.2}/px)", ns, ns / f64::from(len));
        }
        println!();
    }
}

fn setup_table(bank: &SpriteBank) {
    println!("\nPer-sprite cost with 4096 small sprites at random places, best level");
    println!("{:<30}{:>14}{:>16}", "", "ns per sprite", "ns per bbox px");
    let mut rng = Rng::new(11);
    let cases: [(&str, f32, bool); 6] = [
        ("off screen (reject only)", 3.0, true),
        ("2 px", 1.0, false),
        ("4 px", 2.0, false),
        ("8 px", 4.0, false),
        ("8 px, fully occluded", 4.0, false),
        ("16 px", 8.0, false),
    ];
    for (name, radius, off) in cases {
        let occluded = name.contains("occluded");
        let mut draws = ParticleDraws::default();
        for _ in 0..4096 {
            let x = if off {
                -500.0
            } else {
                rng.range_f32(20.0, 620.0)
            };
            let y = rng.range_f32(20.0, 340.0);
            draws
                .add
                .push(sprite(x, y, radius, if occluded { 0.1 } else { 0.9 }));
        }
        let mut fb = if occluded { walled() } else { open() };
        let ns = ns_per_call(50, || {
            draw_particles_with(Level::best(), black_box(&mut fb), black_box(&draws), bank)
        }) / 4096.0;
        let bbox = 4.0 * f64::from(radius) * f64::from(radius);
        println!("{name:<30}{ns:>14.1}{:>16.2}", ns / bbox);
    }
}

fn main() {
    let bank = SpriteBank::new();
    println!(
        "640x360, one thread, best level here: {:?}\n",
        Level::best()
    );
    fill_table(&bank);
    streak_table(&bank);
    setup_table(&bank);
}

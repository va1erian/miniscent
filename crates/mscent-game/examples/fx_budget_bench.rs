//! What the budget buys on a crowded frame: 40 drone explosions before the
//! camera, one thread, baseline kernels' machine choice.
//!
//! Run: `cargo run --release -p mscent-game --example fx_budget_bench`
//!
//! `ns/unit` is the check on the weights: every unit is meant to cost what one
//! additive sprite pixel costs (0.55 ns on SSE2, 0.45 ns on AVX2).

use std::time::Instant;

use mscent_game::Arena;
use mscent_game::fx::budget::{Limits, cost_of};
use mscent_game::fx::combat::{CombatFx, DRONE_TINT};
use mscent_math::Vec3;
use mscent_render::particles::{ParticleDraws, SpriteBank, draw_particles};
use mscent_render::{Framebuffer, Quality, View};
use mscent_world::showcase_level;

const W: u32 = 640;
const H: u32 = 360;
const EXPLOSIONS: usize = 40;
const ROUNDS: u32 = 40;
const FRAME_NS: u64 = 16_666_667;

fn run(name: &str, quality: Quality, arena: &Arena) {
    let view = View::from_angles(Vec3::ZERO, 0.0, 0.0, 0.0);
    let cam = view.camera(1.3, 0.1, 400.0, W, H).expect("a camera");
    let mut fx = CombatFx::new();
    fx.set_quality(quality);
    for i in 0..EXPLOSIONS {
        let spread = i as f32 / EXPLOSIONS as f32 - 0.5;
        let at = Vec3::new(spread * 16.0, 8.0 + spread * 4.0, spread * 6.0);
        fx.drone_explosion(at, DRONE_TINT);
    }
    // Past the delayed ring and into the smoke, near the frame's fullest.
    for _ in 0..12 {
        fx.advance(FRAME_NS, &arena.bsp);
    }
    let (bank, mut draws) = (SpriteBank::new(), ParticleDraws::default());
    let mut fb = Framebuffer::new(W, H).expect("a target");
    let (mut collect_ns, mut draw_ns) = (0u128, 0u128);
    for _ in 0..ROUNDS {
        let start = Instant::now();
        fx.collect(&cam, None, None, &mut draws);
        collect_ns += start.elapsed().as_nanos();
        fb.clear(0xff10_1010);
        let start = Instant::now();
        draw_particles(&mut fb, &draws, &bank);
        draw_ns += start.elapsed().as_nanos();
    }
    let units = cost_of(&draws, None, W as f32, H as f32);
    let screen = (W * H) as f32;
    let rounds = f64::from(ROUNDS);
    let drawn = draw_ns as f64 / rounds;
    println!(
        "{name:<8} live {:>4}  sprites {:>4}  streaks {:>4}  {:>5.2} of {:>3.1} screens  \
         collect {:>6.1} us  draw {:>6.2} ms  {:>5.2} ns/unit",
        fx.live(),
        draws.over.len() + draws.add.len(),
        draws.streaks.len(),
        units / screen,
        Limits::of(quality).screens,
        collect_ns as f64 / rounds / 1000.0,
        drawn / 1.0e6,
        drawn / f64::from(units),
    );
}

fn main() {
    let arena = Arena::from_level(&showcase_level()).expect("showcase loads");
    run("High", Quality::High, &arena);
    run("Medium", Quality::Medium, &arena);
    run("Low", Quality::Low, &arena);
}

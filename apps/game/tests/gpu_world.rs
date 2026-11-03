//! The game's world on the GPU, by readback: the same pictures and the same
//! particles as software, and software where the GPU fails, unless it was the
//! only renderer asked for. A machine with no GPU skips this, unless
//! `MSCENT_REQUIRE_GPU` is set.
#![cfg(feature = "hardware")]

use std::time::Duration;

use miniscent_game::app::{RunOptions, Software, World, run_with};
use miniscent_game::gpu_world::GpuWorld;
use miniscent_game::{GameSession, SessionConfig, assets, levels};
use mscent_gpu::{Gpu, GpuError};
use mscent_math::epsilon::PARITY_MEAN_LEVELS;
use mscent_platform::{HeadlessPlatform, Platform, Script};
use mscent_render::Quality;

const SIZE: (u32, u32) = (320, 180);
/// Blasts set off ahead of the player at the start.
const BLASTS: usize = 8;
const DT: Duration = Duration::from_micros(16_667);
/// Shots down the corridor from the first frames, with the blasts, so sparks,
/// flashes and decals are on screen while the frames are compared.
const FIRE: &str = "at 0.1s fire for 1s\nat 0.3s hold W for 1s\n";

fn gpu() -> Option<Gpu> {
    match Gpu::new() {
        Ok(gpu) => Some(gpu),
        Err(why @ (GpuError::NoAdapter(_) | GpuError::NoDevice(_))) => {
            assert!(
                std::env::var_os("MSCENT_REQUIRE_GPU").is_none(),
                "MSCENT_REQUIRE_GPU is set: {why}"
            );
            eprintln!("skipped: {why}");
            None
        }
        Err(why) => panic!("{why}"),
    }
}

/// At Medium, so the GPU draws the reference finish that parity holds it to.
fn session() -> GameSession {
    let cfg = SessionConfig {
        quality: Quality::Medium,
        ..SessionConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    GameSession::new(&levels::corridor_level(), materials, &cfg).expect("a level")
}

fn platform(frames: u64) -> HeadlessPlatform {
    let script = Script::parse(FIRE).expect("a script");
    HeadlessPlatform::new(SIZE.0, SIZE.1, DT, script).with_frame_limit(frames)
}

/// Each frame's particle draws, whether it had any, and the last frame's
/// pixels, stepping the session as the app does.
fn play(world: &mut dyn World, frames: u64) -> (Vec<(bool, String)>, Vec<u32>) {
    let (mut s, mut p) = (session(), platform(frames));
    // More than the budget holds, so what its depth estimate hides is what
    // decides which particles are thinned.
    let at = s.game().player.pos();
    for i in 0..BLASTS {
        let ahead = mscent_math::Vec3::new(0.0, 2.0 + i as f32, 0.0);
        s.blast(at + ahead, 12.0);
    }
    let mut draws = Vec::new();
    let mut pixels = Vec::new();
    for _ in 0..frames {
        let events = p.poll_events();
        s.step(DT, &events);
        let fb = s.render_through(SIZE.0, SIZE.1, &mut |assets, fb, frame| {
            world.draw(assets, fb, frame)
        });
        pixels = fb.expect("a frame").pixels().to_vec();
        let listed = s.particle_draws();
        draws.push((!listed.is_empty(), format!("{listed:?}")));
    }
    (draws, pixels)
}

/// The mean error per channel of `a` against `b`, each colour of `a` against
/// its nearest within a pixel, as `crates/mscent-gpu/tests/parity.rs` measures:
/// software corrects perspective every 16 pixels and a GPU at each one.
fn mean_error(a: &[u32], b: &[u32]) -> f64 {
    let (w, h) = (SIZE.0 as usize, SIZE.1 as usize);
    let channel_error = |x: u32, y: u32| -> i64 {
        [16, 8, 0]
            .iter()
            .map(|s| i64::from(((x >> s) & 255).abs_diff((y >> s) & 255)))
            .sum()
    };
    let nearest = |i: usize| {
        let (x, y) = (i % w, i / w);
        let rows = y.saturating_sub(1)..=(y + 1).min(h - 1);
        rows.flat_map(|ny| (x.saturating_sub(1)..=(x + 1).min(w - 1)).map(move |nx| ny * w + nx))
            .map(|j| channel_error(a[i], b[j]))
            .min()
            .unwrap_or(0)
    };
    let sum: i64 = (0..a.len().min(w * h)).map(nearest).sum();
    sum as f64 / (a.len() * 3).max(1) as f64
}

#[test]
fn a_fight_on_the_gpu_is_software_s_picture_and_particles() {
    let Some(gpu) = gpu() else { return };
    let frames = 90;
    let (soft_draws, soft) = play(&mut Software, frames);
    let mut world = GpuWorld::on(gpu, true);
    let (gpu_draws, hard) = play(&mut world, frames);
    assert_eq!(world.stop(), None);
    let drawn = soft_draws.iter().filter(|d| d.0).count();
    assert!(drawn > 10, "the fight draws particles: {drawn} frames");
    let first_apart = soft_draws.iter().zip(&gpu_draws).position(|(a, b)| a != b);
    assert_eq!(first_apart, None, "the particle draws part");
    // Never the same bytes: the GPU rounds differently, so this frame came from it.
    assert_ne!(soft, hard, "the GPU drew the world");
    let error = mean_error(&soft, &hard);
    eprintln!("{drawn} frames with particles, the last {error:.3} levels apart");
    assert!(error < f64::from(PARITY_MEAN_LEVELS), "{error}");
}

#[test]
fn a_lost_gpu_leaves_the_world_to_software() {
    let Some(gpu) = gpu() else { return };
    let opts = RunOptions {
        base: SIZE,
        adaptive: false,
        cap_fps: None,
        autoclose: None,
        shot_times: vec![],
        shot_stem: None,
        watch: None,
    };
    let mut world = GpuWorld::on(gpu.clone(), false);
    gpu.destroy();
    let mut s = session();
    let report = run_with(&mut platform(20), &mut s, &opts, &mut world);
    assert_eq!(
        (report.frames, report.stopped),
        (20, None),
        "software drew it"
    );

    let gpu = self::gpu().expect("the GPU again");
    let mut world = GpuWorld::on(gpu.clone(), true);
    gpu.destroy();
    let report = run_with(&mut platform(20), &mut session(), &opts, &mut world);
    assert!(
        report.stopped.is_some(),
        "--renderer hardware stops instead"
    );
    assert!(report.frames < 20, "{} frames", report.frames);
}

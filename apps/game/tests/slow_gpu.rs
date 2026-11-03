//! On a GPU too slow for its window, the world drawn there shrinks: the frame's
//! cost is how long showing it took, and the adaptive resolution reads it. A
//! stand-in GPU that takes 30 ms a frame shows it, with no GPU needed.

use std::time::Duration;

use miniscent_game::app::{RunOptions, Shown, World, run_with};
use miniscent_game::{GameSession, SessionConfig, assets, levels};
use mscent_platform::{HeadlessPlatform, Script};
use mscent_render::{FrameInput, Framebuffer, LevelAssets};

/// Shows every frame, taking `cost` to, and keeps the size of each world.
struct Slow {
    cost: Duration,
    worlds: Vec<(u32, u32)>,
}

impl World for Slow {
    fn draw(&mut self, _: &LevelAssets, _: &mut Framebuffer, _: &FrameInput) -> bool {
        false
    }

    fn shows(&self) -> bool {
        true
    }

    fn show(&mut self, _: &LevelAssets, _: &FrameInput, over: &Shown) -> bool {
        std::thread::sleep(self.cost);
        self.worlds.push(over.world);
        true
    }
}

fn run(cost: Duration) -> Vec<(u32, u32)> {
    let materials = assets::load_materials(None).expect("textures");
    let cfg = SessionConfig::default();
    let mut s = GameSession::new(&levels::corridor_level(), materials, &cfg).expect("a level");
    let script = Script::parse("").expect("a script");
    let dt = Duration::from_micros(16_667);
    // A world that shows its frames presents none, which a frame limit counts,
    // so the run ends by the platform's clock, a frame a poll.
    let mut p = HeadlessPlatform::new(1920, 1080, dt, script);
    let opts = RunOptions {
        base: (640, 360),
        adaptive: true,
        cap_fps: None,
        autoclose: Some(dt * 40),
        shot_times: vec![],
        shot_stem: None,
        watch: None,
    };
    let mut world = Slow {
        cost,
        worlds: Vec::new(),
    };
    run_with(&mut p, &mut s, &opts, &mut world);
    world.worlds
}

#[test]
fn a_gpu_too_slow_for_the_window_draws_a_smaller_world() {
    let slow = run(Duration::from_millis(30));
    assert_eq!(
        slow.first(),
        Some(&(1920, 1080)),
        "the window's own size first"
    );
    let last = *slow.last().expect("frames");
    assert!(last.0 < 1920 && last.1 < 1080, "shrunk to {last:?}");
    let quick = run(Duration::ZERO);
    assert!(
        quick.iter().all(|&s| s == (1920, 1080)),
        "a quick GPU keeps the window's size"
    );
}

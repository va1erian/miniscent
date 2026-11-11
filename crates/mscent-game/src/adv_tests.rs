//! Adversarial regression tests for the game crate.

use crate::test_util::{far_goal, game, open_level, random_trace, start, v};
use crate::{Game, HudModel, Input, Phase, draw_hud};
use mscent_render::Framebuffer;

fn extreme_model(health: i32, max_health: i32) -> HudModel {
    HudModel {
        health,
        max_health,
        enemies_left: usize::MAX,
        goal_distance: f32::NAN,
        speed: u32::MAX,
        hurt: true,
        phase: Phase::Playing,
        use_prompt: None,
        craft: Default::default(),
    }
}

#[test]
fn hud_survives_extreme_models() {
    let mut fb = Framebuffer::new(64, 48).unwrap();
    for (h, m) in [
        (i32::MAX, 100),
        (i32::MIN, 100),
        (100, i32::MIN),
        (100, 0),
        (i32::MAX, i32::MAX),
    ] {
        let model = extreme_model(h, m);
        let _ = model.bar_fill();
        draw_hud(&mut fb, &model);
    }
}

#[test]
fn hud_draws_on_tiny_framebuffers() {
    for (w, h) in [(1, 1), (2, 3), (7, 5)] {
        let mut fb = Framebuffer::new(w, h).unwrap();
        draw_hud(&mut fb, &extreme_model(50, 100));
    }
}

#[test]
fn restart_discards_the_leftover_frame_time() {
    let level = open_level(vec![start(v(0.0, 0.0, 0.0), 0.0), far_goal()]);
    let mut fresh: Game = game(&level);
    let mut used: Game = game(&level);
    // 5 ms is 0.6 of a step: no step runs, but time is banked.
    assert_eq!(used.update(5_000_000, &Input::default()), 0);
    used.restart();
    let steps = |g: &mut Game| {
        (0..6)
            .map(|_| g.update(5_000_000, &Input::default()))
            .collect::<Vec<_>>()
    };
    assert_eq!(steps(&mut used), steps(&mut fresh));
}

#[test]
fn random_input_never_leaves_the_room_or_goes_non_finite() {
    let level = open_level(vec![start(v(0.0, 0.0, 0.0), 0.0), far_goal()]);
    for seed in 0..5 {
        let mut g = game(&level);
        g.run_trace(&random_trace(seed, 6000));
        let p = g.player.pos();
        assert!(p.is_finite() && p.x.abs() < 20.0 && p.y.abs() < 20.0 && p.z.abs() < 20.0);
        let q = g.player.orientation;
        let n = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
        assert!((n - 1.0).abs() < 1e-4, "{n}");
    }
}

//! The particle budget thins the same particles whichever way a frame is drawn:
//! its depth is an estimate the session draws on the CPU, not the frame's own,
//! which a renderer that shows the frame itself never reads back.

use std::time::Duration;

use miniscent_game::{GameSession, SessionConfig, assets, levels};
use mscent_math::Vec3;
use mscent_platform::{HeadlessPlatform, Platform, Script};
use mscent_render::Quality;

const SIZE: (u32, u32) = (320, 180);
const DT: Duration = Duration::from_micros(16_667);

/// A fight past the budget: blasts ahead of the player and shots into them.
fn fight() -> (GameSession, HeadlessPlatform) {
    let cfg = SessionConfig {
        quality: Quality::Medium,
        ..SessionConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    let mut s = GameSession::new(&levels::corridor_level(), materials, &cfg).expect("a level");
    let at = s.game().player.pos();
    for i in 0..8 {
        s.blast(at + Vec3::new(0.0, 2.0 + i as f32, 0.0), 12.0);
    }
    let script = Script::parse("at 0.1s fire for 1s\nat 0.3s hold W for 1s\n").expect("a script");
    (
        s,
        HeadlessPlatform::new(SIZE.0, SIZE.1, DT, script).with_frame_limit(60),
    )
}

#[test]
fn a_frame_shown_by_its_renderer_thins_the_particles_software_does() {
    let (mut soft, mut p) = fight();
    let (mut shown, mut q) = fight();
    let mut thinned = 0;
    for frame in 0..60 {
        let (a, b) = (p.poll_events(), q.poll_events());
        soft.step(DT, &a);
        shown.step(DT, &b);
        soft.render(SIZE.0, SIZE.1).expect("a frame");
        let layered = shown.render_layered((SIZE, SIZE), &mut |_, _, _, _| true);
        assert!(
            matches!(layered, Some(None)),
            "the stand-in showed frame {frame}"
        );
        let (x, y) = (soft.particle_draws(), shown.particle_draws());
        assert_eq!(format!("{x:?}"), format!("{y:?}"), "frame {frame}");
        thinned += usize::from(!x.is_empty());
    }
    assert!(thinned > 30, "the fight drew particles: {thinned} frames");
}

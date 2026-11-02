use super::*;
use crate::test_util::{FACE_PLUS_X, far_goal, game, open_level, start, v};
use mscent_render::View;
use mscent_world::materials::{BRICK, FLOOR, LIGHTPANEL, METAL, ROCK};

const FRAME_NS: u64 = 16_666_667;

fn bsp_game() -> crate::Game {
    game(&open_level(vec![
        start(v(0.0, 0.0, 0.0), FACE_PLUS_X),
        far_goal(),
    ]))
}

fn wall_shot(material: u16) -> ShotFx {
    ShotFx {
        by_player: true,
        muzzle: v(0.0, 0.0, 0.0),
        end: v(19.9, 0.0, 0.0),
        normal: v(-1.0, 0.0, 0.0),
        impact: Impact::Surface {
            material,
            tint: 0xff80_4020,
        },
    }
}

fn run(fx: &mut CombatFx, frames: u32) {
    let g = bsp_game();
    for _ in 0..frames {
        fx.advance(FRAME_NS, &g.arena.bsp);
    }
}

#[test]
fn the_impact_waits_for_the_bolt_and_the_bolt_then_goes() {
    let mut fx = CombatFx::new();
    fx.shot(&wall_shot(METAL));
    assert_eq!((fx.bolts(), fx.live()), (1, 1), "only the muzzle flash yet");
    run(&mut fx, 1);
    assert_eq!(fx.bolts(), 1, "20 units is 50 ms of flight");
    run(&mut fx, 3);
    assert_eq!(fx.bolts(), 0);
    assert!(fx.live() >= 16, "sparks arrived: {}", fx.live());
}

#[test]
fn flight_is_capped_at_any_range() {
    let mut fx = CombatFx::new();
    fx.shot(&ShotFx {
        end: v(19.9, 0.0, 0.0) * 1000.0,
        impact: Impact::None,
        ..wall_shot(METAL)
    });
    run(&mut fx, 5);
    assert_eq!(fx.bolts(), 0, "80 ms is under 5 frames");
}

#[test]
fn each_material_gives_its_own_particles() {
    let counts: Vec<usize> = [METAL, BRICK, ROCK, LIGHTPANEL, FLOOR, 999]
        .iter()
        .map(|&m| {
            let mut fx = CombatFx::new();
            fx.shot(&wall_shot(m));
            run(&mut fx, 4);
            let mut by_class = [0usize; recipes::CLASSES.len()];
            fx.fx.for_each(|p| by_class[usize::from(p.class.0)] += 1);
            by_class
                .iter()
                .enumerate()
                .fold(0, |a, (i, n)| a * 31 + i * n)
        })
        .collect();
    for (i, a) in counts.iter().enumerate() {
        for b in &counts[i + 1..] {
            assert_ne!(a, b, "{counts:?}");
        }
    }
}

#[test]
fn hostile_shots_do_not_panic_and_leave_nothing_non_finite() {
    let bad = [f32::NAN, f32::INFINITY, -f32::INFINITY, 1.0e30];
    let mut fx = CombatFx::new();
    for &x in &bad {
        for impact in [
            Impact::None,
            Impact::Drone,
            Impact::Surface {
                material: 65535,
                tint: 0,
            },
        ] {
            let p = v(x, 0.0, 0.0);
            fx.shot(&ShotFx {
                by_player: false,
                muzzle: p,
                end: v(1.0, 1.0, 1.0),
                normal: p,
                impact,
            });
            fx.shot(&ShotFx {
                by_player: true,
                muzzle: v(0.0, 0.0, 0.0),
                end: p,
                normal: v(1.0, 0.0, 0.0),
                impact,
            });
            fx.drone_explosion(p, 0);
        }
    }
    run(&mut fx, 30);
    fx.fx.for_each(|p| assert!(p.pos.is_finite()));
}

#[test]
fn more_shots_than_bolts_still_land() {
    let mut fx = CombatFx::new();
    for _ in 0..MAX_BOLTS + 8 {
        fx.shot(&wall_shot(METAL));
    }
    assert_eq!(fx.bolts(), MAX_BOLTS);
    assert!(
        fx.live() > MAX_BOLTS + 8,
        "the overflow's impacts spawned now"
    );
}

#[test]
fn a_ramp_and_fog_never_put_a_particle_behind_the_eye_in_the_list() {
    let view = View::from_angles(v(0.0, 0.0, 0.0), -core::f32::consts::FRAC_PI_2, 0.0, 0.0);
    let cam = view.camera(1.3, 0.1, 400.0, 320, 180).expect("a camera");
    let mut fx = CombatFx::new();
    // Ahead of the camera, then behind it.
    fx.drone_explosion(v(5.0, 0.0, 0.0), DRONE_TINT);
    let mut draws = ParticleDraws::default();
    fx.collect(&cam, None, &mut draws);
    let ahead = draws.add.len();
    assert!(ahead > 0);
    let mut behind = CombatFx::new();
    behind.drone_explosion(v(-5.0, 0.0, 0.0), DRONE_TINT);
    behind.collect(&cam, None, &mut draws);
    assert_eq!(draws.add.len(), 0);
}

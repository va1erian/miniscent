use super::*;
use mscent_render::MaterialTable;
use mscent_world::{Brush, Op, Portals, compile, extract_faces};

use crate::test_util::v;

const FRAME_NS: u64 = 16_666_667;

fn spec(pos: Vec3, color: f32, life: f32) -> LightSpec {
    LightSpec {
        pos,
        radius: 6.0,
        color: v(color, color, color),
        life,
        lift: Vec3::ZERO,
    }
}

/// A room of 20 around the origin.
fn room() -> Scene {
    let brushes = [Brush::cuboid(
        mscent_math::Vec3d::new(-20.0, -20.0, -20.0),
        mscent_math::Vec3d::new(20.0, 20.0, 20.0),
        Op::Subtract,
        0,
    )
    .expect("a room")];
    let bsp = compile(&brushes).expect("a tree");
    let faces = extract_faces(&bsp, &brushes);
    let portals = Portals::build(&bsp);
    Scene::new(bsp, faces, portals, MaterialTable::default())
}

fn colors(lights: &mut Lights, scene: &Scene) -> Vec<f32> {
    lights.active(scene).iter().map(|l| l.color().x).collect()
}

#[test]
fn a_light_fades_on_the_square_of_what_is_left_and_then_goes() {
    let scene = room();
    let mut lights = Lights::new();
    lights.add(spec(v(0.0, 0.0, 0.0), 2.0, 0.25), None);
    assert_eq!(colors(&mut lights, &scene), [2.0]);
    // A frame in: a little less, along 1 - t, squared.
    lights.advance(FRAME_NS);
    let t = FRAME_NS as f32 / 0.25e9;
    let want = 2.0 * (1.0 - t) * (1.0 - t);
    assert!((colors(&mut lights, &scene)[0] - want).abs() < 1e-4);
    let mut last = f32::MAX;
    for _ in 0..3 {
        lights.advance(FRAME_NS);
        let c = colors(&mut lights, &scene)[0];
        assert!(c < last, "{c} after {last}");
        last = c;
    }
    for _ in 0..12 {
        lights.advance(FRAME_NS);
    }
    assert_eq!(lights.live(), 0, "0.25 s is 15 frames");
    assert!(colors(&mut lights, &scene).is_empty());
}

#[test]
fn no_time_passing_changes_nothing() {
    let scene = room();
    let mut lights = Lights::new();
    lights.add(spec(v(0.0, 0.0, 0.0), 1.0, 0.1), None);
    lights.advance(FRAME_NS);
    let before = colors(&mut lights, &scene);
    for _ in 0..100 {
        lights.advance(0);
    }
    assert_eq!(colors(&mut lights, &scene), before, "a pause holds a light");
}

#[test]
fn a_stall_cannot_burn_a_light_out_unseen() {
    let mut lights = Lights::new();
    lights.add(spec(v(0.0, 0.0, 0.0), 1.0, 0.5), None);
    lights.advance(u64::MAX);
    assert_eq!(lights.live(), 1, "one clamped step is 67 ms of 500");
}

#[test]
fn over_the_limit_the_dimmest_gives_way() {
    let scene = room();
    let mut lights = Lights::new();
    lights.set_limit(2);
    lights.add(spec(v(0.0, 0.0, 0.0), 1.0, 1.0), None);
    lights.add(spec(v(1.0, 0.0, 0.0), 0.3, 1.0), None);
    lights.add(spec(v(2.0, 0.0, 0.0), 0.6, 1.0), None);
    let mut got = colors(&mut lights, &scene);
    got.sort_by(f32::total_cmp);
    assert_eq!(got, [0.6, 1.0], "the 0.3 gave way to the 0.6");
    // One dimmer than both takes nobody's place.
    lights.add(spec(v(3.0, 0.0, 0.0), 0.1, 1.0), None);
    let mut got = colors(&mut lights, &scene);
    got.sort_by(f32::total_cmp);
    assert_eq!(got, [0.6, 1.0]);
}

#[test]
fn an_old_light_is_dimmer_than_a_new_one_and_gives_way() {
    let scene = room();
    let mut lights = Lights::new();
    lights.set_limit(1);
    lights.add(spec(v(0.0, 0.0, 0.0), 1.0, 0.25), None);
    lights.advance(3 * FRAME_NS);
    lights.add(spec(v(1.0, 0.0, 0.0), 1.0, 0.25), None);
    assert_eq!(colors(&mut lights, &scene), [1.0], "the fresh one, whole");
}

#[test]
fn a_lower_quality_drops_the_dimmest_at_once() {
    let scene = room();
    let mut lights = Lights::new();
    for (i, c) in [0.4, 1.2, 0.8, 0.2].into_iter().enumerate() {
        lights.add(spec(v(i as f32, 0.0, 0.0), c, 1.0), None);
    }
    assert_eq!(lights.live(), 4);
    lights.set_limit(2);
    let mut got = colors(&mut lights, &scene);
    got.sort_by(f32::total_cmp);
    assert_eq!(got, [0.8, 1.2]);
    lights.set_limit(0);
    assert_eq!(lights.live(), 0);
    lights.add(spec(v(0.0, 0.0, 0.0), 1.0, 1.0), None);
    assert_eq!(lights.live(), 0, "a quality with no lights spawns none");
}

#[test]
fn hostile_specs_are_dropped_and_huge_ones_cut_down() {
    let scene = room();
    let nan = f32::NAN;
    let inf = f32::INFINITY;
    let mut lights = Lights::new();
    let base = spec(v(0.0, 0.0, 0.0), 1.0, 0.2);
    let hostile = [
        LightSpec {
            pos: v(nan, 0.0, 0.0),
            ..base
        },
        LightSpec {
            pos: v(0.0, inf, 0.0),
            ..base
        },
        LightSpec {
            radius: nan,
            ..base
        },
        LightSpec {
            radius: inf,
            ..base
        },
        LightSpec {
            radius: 0.0,
            ..base
        },
        LightSpec {
            radius: -1.0,
            ..base
        },
        LightSpec { life: nan, ..base },
        LightSpec { life: 0.0, ..base },
        LightSpec { life: -1.0, ..base },
        LightSpec { life: inf, ..base },
        LightSpec {
            color: v(-1.0, 1.0, 1.0),
            ..base
        },
        LightSpec {
            color: v(nan, 1.0, 1.0),
            ..base
        },
        LightSpec {
            color: Vec3::ZERO,
            ..base
        },
        LightSpec {
            lift: v(nan, 0.0, 0.0),
            ..base
        },
    ];
    for h in hostile {
        lights.add(h, None);
    }
    assert_eq!(lights.live(), 0);
    lights.add(
        LightSpec {
            radius: 1.0e30,
            color: v(1.0e30, 1.0, 1.0),
            life: 1.0e30,
            ..base
        },
        None,
    );
    assert_eq!(lights.live(), 1);
    let l = &lights.active(&scene)[0];
    assert_eq!(
        (l.radius(), l.color().x),
        (
            mscent_render::dynlight::MAX_RADIUS,
            mscent_render::dynlight::MAX_COLOR
        )
    );
    // Ten seconds at the most: alive after five, gone after twelve.
    for _ in 0..300 {
        lights.advance(FRAME_NS);
    }
    assert_eq!(lights.live(), 1);
    for _ in 0..420 {
        lights.advance(FRAME_NS);
    }
    assert_eq!(lights.live(), 0);
}

#[test]
fn a_lift_that_ends_in_solid_is_halved() {
    let g = crate::test_util::game(&crate::test_util::open_level(vec![
        crate::test_util::start(v(0.0, 0.0, 0.0), crate::test_util::FACE_PLUS_X),
        crate::test_util::far_goal(),
    ]));
    let at = v(19.9, 0.0, 0.0);
    let wall = LightSpec {
        lift: v(-0.6, 0.0, 0.0),
        ..spec(at, 1.0, 1.0)
    };
    let mut lights = Lights::new();
    lights.add(wall, Some(&g.arena.bsp));
    assert!(
        (lights.life[0].spec.pos.x - 19.3).abs() < 1e-4,
        "lifted into the room"
    );
    // Lifted the wrong way, into the rock: it tries less, then stays put.
    let into = LightSpec {
        lift: v(0.6, 0.0, 0.0),
        ..spec(at, 1.0, 1.0)
    };
    lights.clear();
    lights.add(into, Some(&g.arena.bsp));
    assert_eq!(lights.life[0].spec.pos, at);
}

#[test]
fn the_same_adds_and_steps_give_the_same_lights() {
    let scene = room();
    let run = || {
        let mut lights = Lights::new();
        let mut out = Vec::new();
        for i in 0..12 {
            if i % 3 == 0 {
                lights.add(
                    spec(
                        v(i as f32, 1.0, 0.0),
                        1.0 + i as f32 * 0.1,
                        0.1 + i as f32 * 0.02,
                    ),
                    None,
                );
            }
            lights.advance(FRAME_NS);
            out.push(colors(&mut lights, &scene));
        }
        out
    };
    assert_eq!(run(), run());
}

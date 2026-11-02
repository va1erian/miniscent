use super::*;
use crate::fx::combat::{CombatFx, Impact, ShotFx};
use crate::fx::recipes::surface_mark;
use crate::test_util::{FACE_PLUS_X, far_goal, game, open_level, start, v};
use mscent_render::particles::ParticleDraws;
use mscent_render::{Fog, View};
use mscent_world::materials::{BRICK, METAL};
use mscent_world::{Brush, Op, compile, extract_faces};

use crate::fx::budget::{Sink, Tags};

const FRAME_NS: u64 = 16_666_667;
const CAP_SCREENS: f32 = 0.25;
const MARK: Mark = Mark {
    radius: 0.5,
    darkness: 200,
};

/// The six faces of a 40-unit room, as outlines.
fn room_faces() -> Vec<(Face, FaceOutline)> {
    let room = Brush::cuboid(
        Vec3d::new(-40.0, -40.0, -40.0),
        Vec3d::new(40.0, 40.0, 40.0),
        Op::Subtract,
        0,
    )
    .expect("a room");
    let bsp = compile(std::slice::from_ref(&room)).expect("compiles");
    extract_faces(&bsp, &[room])
        .into_iter()
        .map(|f| {
            let outline = FaceOutline::of(&f).expect("an outline");
            (f, outline)
        })
        .collect()
}

fn floor() -> FaceOutline {
    let faces = room_faces();
    let pick = faces.iter().find(|(f, _)| f.plane.normal.z > 0.9);
    pick.expect("a floor").1
}

#[test]
fn the_ring_replaces_the_oldest_and_keeps_sixty_four() {
    let mut ring = Decals::new();
    let at = |i: usize| Vec3::new(i as f32, 0.0, 0.0);
    for i in 0..MAX_DECALS {
        let d = Decal::new(at(i), Vec3::new(0.0, 0.0, 1.0), MARK, None).expect("a decal");
        ring.push(d);
    }
    assert_eq!(ring.len(), 64);
    let x = |ring: &Decals| {
        ring.iter()
            .map(|d| d.centre().x.round())
            .collect::<Vec<_>>()
    };
    assert_eq!(x(&ring).first(), Some(&0.0));
    let d65 = Decal::new(at(64), Vec3::new(0.0, 0.0, 1.0), MARK, None).expect("a decal");
    ring.push(d65);
    let after = x(&ring);
    assert_eq!(ring.len(), 64, "still 64");
    assert_eq!((after.first(), after.last()), (Some(&1.0), Some(&64.0)));
    assert!(!after.contains(&0.0), "the first is gone");
    ring.clear();
    assert!(ring.is_empty() && ring.iter().next().is_none());
}

#[test]
fn a_decal_is_a_square_on_the_plane_facing_out() {
    let n = Vec3::new(0.0, 0.0, 1.0);
    let d = Decal::new(Vec3::new(3.0, 4.0, 5.0), n, MARK, None).expect("a decal");
    assert_eq!(d.vertices().len(), 4);
    for p in d.vertices() {
        assert!((p.pos.z - 5.0).abs() < 1.0e-5, "in the plane");
        let off = p.pos - Vec3::new(3.0, 4.0, 5.0);
        assert!((off.length() - MARK.radius * 2.0_f32.sqrt()).abs() < 1.0e-4);
    }
    // Counter-clockwise seen from the front, as the renderer's facing test
    // expects: the first edge crossed with the second points along the normal.
    let w = d.vertices();
    let turn = (w[1].pos - w[0].pos).cross(w[2].pos - w[1].pos);
    assert!(turn.dot(n) > 0.0);
}

#[test]
fn hostile_positions_and_normals_make_no_decal() {
    let up = Vec3::new(0.0, 0.0, 1.0);
    let bad = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
    for b in bad {
        assert!(Decal::new(Vec3::new(b, 0.0, 0.0), up, MARK, None).is_none());
        assert!(Decal::new(Vec3::ZERO, Vec3::new(0.0, b, 0.0), MARK, None).is_none());
    }
    assert!(Decal::new(Vec3::ZERO, Vec3::ZERO, MARK, None).is_none());
    // Huge but finite values must not panic, whatever they produce.
    let huge = Vec3::new(3.0e38, 3.0e38, 3.0e38);
    let _ = Decal::new(huge, up, MARK, None);
    let _ = Decal::new(Vec3::ZERO, huge, MARK, floor_ref());
    let wild = Mark {
        radius: f32::MAX,
        darkness: u16::MAX,
    };
    let _ = Decal::new(Vec3::ZERO, up, wild, None);
}

fn floor_ref() -> Option<&'static FaceOutline> {
    use std::sync::OnceLock;
    static FLOOR: OnceLock<FaceOutline> = OnceLock::new();
    Some(FLOOR.get_or_init(floor))
}

#[test]
fn a_decal_at_an_edge_is_cut_to_the_face() {
    let outline = floor();
    let up = Vec3::new(0.0, 0.0, 1.0);
    let at = Vec3::new(39.9, 0.0, -40.0);
    let free = Decal::new(at, up, MARK, None).expect("free");
    assert!(free.vertices().iter().any(|p| p.pos.x > 40.0), "spills");
    let cut = Decal::new(at, up, MARK, Some(&outline)).expect("cut");
    assert!(cut.vertices().len() >= 3);
    for p in cut.vertices() {
        assert!(p.pos.x <= 40.0 + 1.0e-4, "x = {}", p.pos.x);
        assert!((p.pos.z + 40.0).abs() < 1.0e-4, "on the floor");
        assert!((0.0..=1.0).contains(&p.s) && (0.0..=1.0).contains(&p.t));
    }
    // Cut in a corner as well: two edges.
    let corner = Decal::new(Vec3::new(39.9, 39.9, -40.0), up, MARK, Some(&outline));
    let corner = corner.expect("corner");
    assert!(
        corner
            .vertices()
            .iter()
            .all(|p| p.pos.x <= 40.0001 && p.pos.y <= 40.0001)
    );
}

#[test]
fn the_face_plane_wins_over_a_shot_that_ended_off_it() {
    let outline = floor();
    let off = Vec3::new(0.0, 0.0, -39.96);
    let d = Decal::new(off, Vec3::new(0.0, 0.0, 1.0), MARK, Some(&outline)).expect("a decal");
    assert!(d.vertices().iter().all(|p| (p.pos.z + 40.0).abs() < 1.0e-4));
}

#[test]
fn a_mark_wholly_off_its_face_is_refused() {
    let outline = floor();
    let away = Vec3::new(100.0, 0.0, -40.0);
    assert!(Decal::new(away, Vec3::new(0.0, 0.0, 1.0), MARK, Some(&outline)).is_none());
}

#[test]
fn every_room_face_gives_an_outline_of_four_edges() {
    let faces = room_faces();
    assert_eq!(faces.len(), 6);
    assert!(faces.iter().all(|(_, o)| o.edges().len() == 4));
}

#[test]
fn materials_choose_the_size_and_darkness() {
    assert!(surface_mark(METAL).radius < surface_mark(BRICK).radius);
    for m in [METAL, BRICK, 9999, u16::MAX] {
        let mark = surface_mark(m);
        assert!(mark.radius > 0.0 && (1..=256).contains(&mark.darkness));
    }
}

fn bsp_game() -> crate::Game {
    game(&open_level(vec![
        start(v(0.0, 0.0, 0.0), FACE_PLUS_X),
        far_goal(),
    ]))
}

fn wall_shot() -> ShotFx {
    ShotFx {
        by_player: true,
        muzzle: v(0.0, 0.0, 0.0),
        end: v(19.9, 0.0, 0.0),
        normal: v(-1.0, 0.0, 0.0),
        impact: Impact::Surface {
            material: BRICK,
            tint: 0xff80_4020,
        },
    }
}

#[test]
fn the_mark_appears_when_the_bolt_arrives_and_not_when_it_is_fired() {
    let g = bsp_game();
    let mut fx = CombatFx::new();
    fx.shot(&wall_shot());
    assert_eq!((fx.bolts(), fx.decals()), (1, 0), "in flight, no mark yet");
    for _ in 0..8 {
        fx.advance(FRAME_NS, &g.arena.bsp);
    }
    assert_eq!((fx.bolts(), fx.decals()), (0, 1));
    fx.clear();
    assert_eq!(fx.decals(), 0, "clear removes them");
}

#[test]
fn only_a_wall_leaves_a_mark() {
    let g = bsp_game();
    let mut fx = CombatFx::new();
    let mut shot = wall_shot();
    shot.impact = Impact::Drone;
    fx.shot(&shot);
    shot.impact = Impact::None;
    fx.shot(&shot);
    for _ in 0..8 {
        fx.advance(FRAME_NS, &g.arena.bsp);
    }
    assert_eq!(fx.decals(), 0);
}

#[test]
fn with_every_bolt_slot_taken_the_mark_still_lands() {
    let mut fx = CombatFx::new();
    for _ in 0..64 {
        fx.shot(&wall_shot());
    }
    assert_eq!(
        (fx.bolts(), fx.decals()),
        (32, 32),
        "the overflow lands at once"
    );
}

#[test]
fn hostile_shots_make_no_mark_and_do_not_panic() {
    let mut fx = CombatFx::new();
    let nan = f32::NAN;
    for end in [
        v(nan, 0.0, 0.0),
        v(f32::INFINITY, 0.0, 0.0),
        v(3.0e38, 0.0, 0.0),
    ] {
        for normal in [v(nan, 0.0, 0.0), Vec3::ZERO, v(0.0, f32::INFINITY, 0.0)] {
            let mut shot = wall_shot();
            shot.end = end;
            shot.normal = normal;
            fx.shot(&shot);
            fx.shot_on(&shot, floor_ref());
        }
    }
    let g = bsp_game();
    for _ in 0..8 {
        fx.advance(FRAME_NS, &g.arena.bsp);
    }
    assert_eq!(fx.decals(), 0);
}

fn collected(decals: &Decals, fog: Option<Fog>) -> ParticleDraws {
    let view = View::from_angles(Vec3::ZERO, 0.0, 0.0, 0.0);
    let cam = view.camera(1.3, 0.1, 400.0, 320, 180).expect("a camera");
    let mut out = ParticleDraws::default();
    let mut tags = Tags::default();
    let mut sink = Sink {
        out: &mut out,
        tags: &mut tags,
        width: 320.0,
        height: 180.0,
    };
    decals.collect(&cam, &FogView(fog), CAP_SCREENS, &mut sink);
    out
}

/// A ring with one mark `ahead` units along +Y, facing the origin.
fn one_ahead(ahead: f32, facing: f32) -> Decals {
    let mut ring = Decals::new();
    let n = Vec3::new(0.0, -facing, 0.0);
    ring.push(Decal::new(Vec3::new(0.0, ahead, 0.0), n, MARK, None).expect("a decal"));
    ring
}

#[test]
fn a_mark_facing_away_is_not_collected() {
    assert_eq!(collected(&one_ahead(10.0, 1.0), None).decals.len(), 1);
    assert!(collected(&one_ahead(10.0, -1.0), None).decals.is_empty());
}

#[test]
fn fog_fades_a_mark_and_swallows_a_distant_one() {
    let clear = collected(&one_ahead(10.0, 1.0), None).decals[0].darkness;
    let hazy = Fog::new(0x808080, 0.0, 20.0);
    let part = collected(&one_ahead(10.0, 1.0), hazy).decals[0].darkness;
    assert!(part > 0 && part < clear, "{part} against {clear}");
    let thick = Fog::new(0x808080, 0.0, 5.0);
    assert!(collected(&one_ahead(10.0, 1.0), thick).decals.is_empty());
}

#[test]
fn marks_piled_up_in_front_of_the_eye_stop_at_the_area_cap() {
    let mut ring = Decals::new();
    let close = Mark {
        radius: 0.3,
        darkness: 200,
    };
    for _ in 0..MAX_DECALS {
        let at = Vec3::new(0.0, 0.6, 0.0);
        let d = Decal::new(at, Vec3::new(0.0, -1.0, 0.0), close, None);
        ring.push(d.expect("a decal"));
    }
    let out = collected(&ring, None);
    let area: f32 = out.decals.iter().map(|d| d.screen_area(320.0, 180.0)).sum();
    assert!(!out.decals.is_empty() && out.decals.len() < MAX_DECALS);
    assert!(area <= CAP_SCREENS * 320.0 * 180.0, "{area}");
}

#[test]
fn a_mark_too_big_for_the_cap_does_not_hide_the_older_ones() {
    let view = View::from_angles(v(0.0, 0.0, 0.0), -core::f32::consts::FRAC_PI_2, 0.0, 0.0);
    let cam = view.camera(1.3, 0.1, 400.0, 640, 360).expect("a camera");
    let mark = |x: f32, half: f32| {
        Decal::new(
            v(x, 0.0, 0.0),
            v(-1.0, 0.0, 0.0),
            Mark {
                radius: half,
                darkness: 200,
            },
            None,
        )
    };
    let mut ring = Decals::new();
    ring.push(mark(10.0, 0.3).expect("a small mark, far off"));
    ring.push(mark(0.5, 0.45).expect("a big mark, right at the eye"));
    let (mut draws, mut tags) = (ParticleDraws::default(), Tags::default());
    let mut sink = Sink {
        out: &mut draws,
        tags: &mut tags,
        width: 640.0,
        height: 360.0,
    };
    ring.collect(&cam, &FogView(None), CAP_SCREENS, &mut sink);
    assert_eq!(draws.decals.len(), 1, "the far one, on its own");
}

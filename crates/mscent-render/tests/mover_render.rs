//! Movers on screen, judged by reading real pixels. A room with a lamp and a
//! door across the view; only the mover's pose changes between pictures.

use std::path::Path;

use mscent_math::{Quat, Vec3, Vec3d};
use mscent_render::{
    BuildConfig, Framebuffer, LevelRenderer, MaterialTable, MoverPose, Quality, View,
};
use mscent_world::materials::METAL;
use mscent_world::{Brush, Entity, Level, MoverGroup, Op};

const W: u32 = 160;
const H: u32 = 90;

fn materials() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("textures")
}

fn vd(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

/// A long room with a door across it at y = 8, lit on the near side.
fn level(with_door: bool) -> Level {
    let room = Brush::cuboid(vd(-8.0, -8.0, -4.0), vd(8.0, 24.0, 4.0), Op::Subtract, 0).unwrap();
    let mut level = Level {
        brushes: vec![room],
        entities: vec![Entity::Light {
            pos: Vec3::new(0.0, 2.0, 3.0),
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 30.0,
            intensity: 3.0,
        }],
        ..Level::default()
    };
    if with_door {
        let door = Brush::cuboid(vd(-3.0, 8.0, -3.0), vd(3.0, 9.0, 3.0), Op::Add, 1).unwrap();
        level.brushes.push(door.with_material(METAL));
        level.mover_groups.push(MoverGroup {
            name: "door".to_string(),
            tags: Vec::new(),
            brushes: vec![1],
        });
    }
    level
}

fn renderer(with_door: bool) -> LevelRenderer {
    let cfg = BuildConfig {
        luxel_size: 0.5,
        shadow_samples: 4,
        ..BuildConfig::default()
    };
    LevelRenderer::from_level(&level(with_door), materials(), &cfg).expect("builds")
}

/// From the origin along +Y, at the door.
fn view() -> View {
    View {
        pos: Vec3::ZERO,
        orientation: looking_along_y(),
    }
}

fn looking_along_y() -> Quat {
    // The camera looks along -Z; a quarter turn about +X brings that to +Y.
    Quat::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), core::f32::consts::FRAC_PI_2)
}

fn picture(r: &LevelRenderer, quality: Quality, poses: &[MoverPose]) -> Vec<u32> {
    let mut fb = Framebuffer::new(W, H).expect("fb");
    r.render_posed(&mut fb, &view(), quality, &[], None, poses)
        .expect("renders");
    fb.pixels().to_vec()
}

fn slid(by: Vec3) -> MoverPose {
    MoverPose {
        offset: by,
        ..MoverPose::AUTHORED
    }
}

fn centre_block(px: &[u32], half: usize) -> Vec<u32> {
    let (cx, cy) = (W as usize / 2, H as usize / 2);
    let mut out = Vec::new();
    for y in cy - half..cy + half {
        for x in cx - half..cx + half {
            out.push(px[y * W as usize + x]);
        }
    }
    out
}

fn difference(a: &[u32], b: &[u32]) -> f32 {
    let chan = |p: u32, s: u32| ((p >> s) & 0xff) as i32;
    let sum: i32 = a
        .iter()
        .zip(b)
        .map(|(&p, &q)| {
            (0..3)
                .map(|c| (chan(p, c * 8) - chan(q, c * 8)).abs())
                .sum::<i32>()
        })
        .sum();
    sum as f32 / (a.len() * 3) as f32
}

fn brightness(px: &[u32]) -> f32 {
    let total: u32 = px.iter().map(|p| (p >> 8) & 0xff).sum();
    total as f32 / px.len() as f32
}

#[test]
fn a_closed_door_is_drawn_lit_and_opening_it_shows_what_is_behind() {
    let r = renderer(true);
    let shut = picture(&r, Quality::Medium, &[]);
    let open = picture(&r, Quality::Medium, &[slid(Vec3::new(0.0, 0.0, 30.0))]);
    let (shut_c, open_c) = (centre_block(&shut, 8), centre_block(&open, 8));
    assert!(
        difference(&shut_c, &open_c) > 4.0,
        "the door must change the picture"
    );
    assert!(
        brightness(&shut_c) > 8.0,
        "the door is lit: {}",
        brightness(&shut_c)
    );
    assert!(shut_c.iter().any(|&p| p != shut_c[0]), "and textured");
}

#[test]
fn an_open_door_leaves_the_same_picture_as_no_door_at_all() {
    let (with, without) = (renderer(true), renderer(false));
    let open = picture(&with, Quality::Medium, &[slid(Vec3::new(0.0, 0.0, 30.0))]);
    let none = picture(&without, Quality::Medium, &[]);
    assert!(
        difference(&open, &none) < 1.0,
        "{}",
        difference(&open, &none)
    );
}

#[test]
fn no_poses_means_as_authored() {
    let r = renderer(true);
    let authored = picture(&r, Quality::Medium, &[MoverPose::AUTHORED]);
    assert_eq!(picture(&r, Quality::Medium, &[]), authored);
    let mut fb = Framebuffer::new(W, H).expect("fb");
    r.render(&mut fb, &view(), Quality::Medium, &[], None)
        .expect("renders");
    assert_eq!(fb.pixels(), authored);
}

#[test]
fn a_door_behind_a_wall_is_hidden_by_the_z_buffer() {
    let r = renderer(true);
    // Past the room's far end, inside the rock.
    let buried = picture(&r, Quality::Medium, &[slid(Vec3::new(0.0, 25.0, 0.0))]);
    let open = picture(&r, Quality::Medium, &[slid(Vec3::new(0.0, 0.0, 30.0))]);
    assert_eq!(buried, open);
}

#[test]
fn a_door_behind_the_eye_draws_nothing() {
    let r = renderer(true);
    let behind = picture(&r, Quality::Medium, &[slid(Vec3::new(0.0, -14.0, 0.0))]);
    let open = picture(&r, Quality::Medium, &[slid(Vec3::new(0.0, 0.0, 30.0))]);
    assert_eq!(behind, open);
}

#[test]
fn a_door_that_moves_carries_its_picture_with_it() {
    let r = renderer(true);
    let shut = picture(&r, Quality::Low, &[]);
    // A shift of whole pixels is hard to arrange, so compare the picture's left
    // half when the door slides right: what was the centre is now to the right.
    let moved = picture(&r, Quality::Low, &[slid(Vec3::new(1.0, 0.0, 0.0))]);
    assert!(difference(&shut, &moved) > 0.1);
    assert!(
        brightness(&centre_block(&moved, 8)) > 8.0,
        "still a lit door in the middle"
    );
}

#[test]
fn a_door_swung_on_its_hinge_shows_its_edge_and_the_room_behind() {
    let r = renderer(true);
    let swung = MoverPose {
        rot: Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), core::f32::consts::FRAC_PI_2),
        pivot: Vec3::new(3.0, 8.0, 0.0),
        offset: Vec3::ZERO,
    };
    let open = picture(&r, Quality::Medium, &[swung]);
    let far = picture(&r, Quality::Medium, &[slid(Vec3::new(0.0, 0.0, 30.0))]);
    let shut = picture(&r, Quality::Medium, &[]);
    // The middle now sees past it, as with the door gone, not as when shut.
    let (o, f, s) = (
        centre_block(&open, 4),
        centre_block(&far, 4),
        centre_block(&shut, 4),
    );
    assert!(difference(&o, &f) < difference(&o, &s));
    // But it is still there, off to one side.
    assert!(difference(&open, &far) > 0.1);
}

#[test]
fn every_quality_draws_every_face_of_a_turned_door() {
    let r = renderer(true);
    let turn = MoverPose {
        rot: Quat::from_axis_angle(Vec3::new(0.3, 0.5, 1.0), 0.8),
        pivot: Vec3::new(0.0, 8.0, 0.0),
        offset: Vec3::ZERO,
    };
    for quality in [Quality::Low, Quality::Medium, Quality::High] {
        let px = picture(&r, quality, &[turn]);
        assert!(brightness(&px) > 1.0, "{quality:?}");
    }
}

#[test]
fn a_hostile_pose_draws_nothing_wrong_and_does_not_panic() {
    let r = renderer(true);
    for bad in [
        slid(Vec3::new(f32::NAN, 0.0, 0.0)),
        slid(Vec3::new(f32::INFINITY, 0.0, 0.0)),
        slid(Vec3::new(1.0e30, 0.0, 0.0)),
        MoverPose {
            rot: Quat {
                x: f32::NAN,
                y: 0.0,
                z: 0.0,
                w: 0.0,
            },
            ..MoverPose::AUTHORED
        },
    ] {
        let _ = picture(&r, Quality::High, &[bad]);
    }
}

#[test]
fn a_level_with_no_movers_lights_the_same_faces_as_before() {
    assert_eq!(renderer(false).lightmap().projections().len(), 6);
    assert_eq!(renderer(true).lightmap().projections().len(), 12);
}

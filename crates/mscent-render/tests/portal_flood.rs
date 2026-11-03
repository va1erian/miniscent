//! The portal flood must not lose a leaf that can be seen. In the showcase's
//! south-east room, a corridor seen past the room's west end came and went as
//! the eye moved: the west end was reached through two thin windows at either
//! edge of the screen first, whose bounding box then covered the wide window
//! that looked into the corridor, which was dropped as seen already. And an eye
//! just off a doorway's plane lost the room past it, as the doorway was clipped at
//! the near plane, nearer than which the rays to that room cross it. Every pixel of
//! a frame from inside a closed room is drawn, so a hole is a lost leaf.

use std::path::Path;

use mscent_math::Vec3;
use mscent_math::rng::Rng;
use mscent_render::{BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Quality, View};
use mscent_world::{LeafKind, showcase_level};

fn renderer() -> LevelRenderer {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    LevelRenderer::from_level(&showcase_level(), materials, &BuildConfig::default())
        .expect("showcase builds")
}

/// Pixels of a frame from `view` where nothing was drawn.
fn holes(r: &LevelRenderer, view: &View, (w, h): (u32, u32)) -> usize {
    let mut fb = Framebuffer::new(w, h).expect("a framebuffer");
    r.render(&mut fb, view, Quality::Low, &[], None)
        .expect("a frame");
    let (w, h) = (w as i32, h as i32);
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| fb.depth_at(x, y) == Some(0.0))
        .count()
}

/// Whether `p` is more than a little off the plane of every face it stands
/// beside: nearer, the near plane cuts the face, which is not a lost leaf.
fn clear_of_faces(r: &LevelRenderer, p: Vec3) -> bool {
    let scene = r.scene();
    scene.faces().iter().zip(scene.polys()).all(|(face, poly)| {
        let axis = |v: Vec3, i: usize| [v.x, v.y, v.z][i];
        let beside = (0..3).all(|i| {
            let along = poly.iter().map(|v| axis(v.pos, i));
            let lo = along.clone().fold(f32::MAX, f32::min);
            let hi = along.fold(f32::MIN, f32::max);
            (lo - 1.0..hi + 1.0).contains(&axis(p, i))
        });
        !beside || face.plane.signed_distance(p.to_f64()).abs() > 0.6
    })
}

#[test]
fn the_corridor_past_the_west_end_is_drawn() {
    let r = renderer();
    let view = View::from_angles(Vec3::new(43.149, 5.267, 4.708), 0.471, -0.038, 0.0);
    for size in [(320, 180), (1280, 720)] {
        assert_eq!(holes(&r, &view, size), 0, "{size:?}");
    }
}

#[test]
fn the_room_past_a_doorway_the_eye_nearly_stands_in_is_drawn() {
    let r = renderer();
    // 0.11 below the doorway at y = 6, just past where the eye counts as in it.
    let view = View::from_angles(Vec3::new(41.294, 5.890, 5.989), 0.536, 0.0, 0.0);
    for size in [(320, 180), (1280, 720)] {
        assert_eq!(holes(&r, &view, size), 0, "{size:?}");
    }
}

#[test]
fn no_pose_in_the_room_shows_a_hole() {
    let r = renderer();
    let bsp = r.scene().bsp();
    let mut rng = Rng::new(11);
    let (mut tried, mut lost) = (0, Vec::new());
    while tried < 400 {
        let p = Vec3::new(
            rng.range_f32(32.0, 50.0),
            rng.range_f32(0.0, 20.0),
            rng.range_f32(0.0, 10.0),
        );
        if bsp.leaf_kind(bsp.leaf_at_f32(p)) != LeafKind::Empty || !clear_of_faces(&r, p) {
            continue;
        }
        tried += 1;
        let (yaw, pitch) = (rng.range_f32(-3.1, 3.1), rng.range_f32(-0.5, 0.5));
        let view = View::from_angles(p, yaw, pitch, 0.0);
        let n = holes(&r, &view, (320, 180));
        if n > 0 {
            lost.push(format!("{p:?} yaw {yaw} pitch {pitch}: {n} pixels"));
        }
    }
    assert!(
        lost.is_empty(),
        "{} of {tried}:\n{}",
        lost.len(),
        lost.join("\n")
    );
}

//! What a vista's room does to the light of the play space, judged on baked
//! luxels, atlas areas and pixels: its faces bake coarsely or not at all, and its
//! lamps do not reach what is lit without shadows.

use std::path::Path;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{
    Actor, ActorKind, BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::lightmap::uv::project_face;
use mscent_world::lightmap::{LightFace, Lightmap};
use mscent_world::materials::{FIRST_SKY, LIGHTPANEL, METAL};
use mscent_world::{Brush, Entity, Level, MAX_VISTA_FOG, Op};

const W: u32 = 160;
const H: u32 = 90;
/// Where the vista's room is.
const AWAY: f64 = 1000.0;
/// The atlas a level may fill: 1024 by 8192 luxels.
const ATLAS_LUXELS: f64 = 1024.0 * 8192.0;

fn materials() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("textures")
}

fn vd(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn cuboid(min: Vec3d, max: Vec3d, op: Op, order: u32, material: u16) -> Brush {
    Brush::cuboid(min, max, op, order)
        .expect("a cuboid")
        .with_material(material)
}

/// The play room: 32 by 64 by 32, its far wall at y = 48.
fn play_room() -> Brush {
    cuboid(
        vd(-16.0, -16.0, -16.0),
        vd(16.0, 48.0, 16.0),
        Op::Subtract,
        0,
        METAL,
    )
}

/// A room `side` across and 32 high, centred on `AWAY`.
fn vista_room(side: f64, material: u16) -> Brush {
    let half = side / 2.0;
    let (lo, hi) = (vd(AWAY - half, -half, -16.0), vd(AWAY + half, half, 16.0));
    cuboid(lo, hi, Op::Subtract, 1, material)
}

/// A grid of pillars, 20 units apart and 16 across, standing in a vista room
/// `side` across: many big faces, as a detailed arena has.
fn pillars(side: f64) -> Vec<Brush> {
    let n = (side / 20.0) as u32 - 1;
    let start = AWAY - f64::from(n) * 10.0;
    let mut out = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (
                start + f64::from(i) * 20.0,
                f64::from(j) * 20.0 - f64::from(n) * 10.0,
            );
            let (lo, hi) = (vd(x, y, -16.0), vd(x + 16.0, y + 16.0, 0.0));
            out.push(cuboid(lo, hi, Op::Add, 2 + i * n + j, METAL));
        }
    }
    out
}

fn lamp(pos: Vec3, radius: f32) -> Entity {
    Entity::Light {
        pos,
        color: Vec3::new(1.0, 1.0, 1.0),
        radius,
        intensity: 1.0,
    }
}

fn layer_at(pos: Vec3) -> Entity {
    Entity::VistaLayer {
        pos,
        centre: Vec3::ZERO,
        slot: 0,
        layer: 0,
        parallax: 0.0,
        spin: Vec3::ZERO,
        pan: [0.0, 0.0],
        fog_start: 0.0,
        fog_end: MAX_VISTA_FOG,
        fog_color: 0,
    }
}

/// Above the pillars, where the room is empty.
fn layer() -> Entity {
    layer_at(Vec3::new(AWAY as f32, 0.0, 8.0))
}

fn play_lamp() -> Entity {
    lamp(Vec3::new(0.0, 16.0, 8.0), 60.0)
}

fn config() -> BuildConfig {
    BuildConfig {
        luxel_size: 0.125,
        shadow_samples: 1,
        light_radius: 0.0,
        fog_start: 500.0,
        fog_end: 1000.0,
        ..BuildConfig::default()
    }
}

fn renderer(brushes: Vec<Brush>, entities: Vec<Entity>) -> LevelRenderer {
    let level = Level {
        brushes,
        entities,
        ..Level::default()
    };
    LevelRenderer::from_level(&level, materials(), &config()).expect("builds")
}

/// Faces of the play room, by their place in the level's face list.
fn play_faces(r: &LevelRenderer) -> Vec<usize> {
    let at = |i: &usize| r.lightmap().projections()[*i].origin.x.abs() < 100.0;
    (0..r.lightmap().projections().len()).filter(at).collect()
}

fn luxels_of(map: &Lightmap, face: usize) -> Vec<(u32, u32, f32, f32, f32)> {
    let p = &map.projections()[face];
    let cells = (0..p.height).flat_map(|y| (0..p.width).map(move |x| (x, y)));
    let at = |(x, y)| {
        let l = map.face_luxel(face, x, y).expect("inside");
        (x, y, l.color.x, l.color.y, l.color.z)
    };
    cells.map(at).collect()
}

fn luxel_count(map: &Lightmap, faces: impl Iterator<Item = usize>) -> u64 {
    let rects = map.atlas().rects();
    faces
        .map(|i| u64::from(rects[i].w) * u64::from(rects[i].h))
        .sum()
}

#[test]
fn a_large_lit_vista_room_leaves_the_play_spaces_luxel_size_alone() {
    let alone = renderer(vec![play_room()], vec![play_lamp()]);
    let mut brushes = vec![play_room(), vista_room(400.0, METAL)];
    brushes.extend(pillars(400.0));
    let corner = Vec3::new(AWAY as f32 - 195.0, -195.0, 8.0);
    let entities = vec![play_lamp(), lamp(corner, 60.0), layer()];
    let with = renderer(brushes, entities);

    // Small faces keep 0.125; the big ones were already grown to the grid cap.
    let grid = |r: &LevelRenderer| {
        let p = r.lightmap().projections();
        let mut sizes: Vec<_> = play_faces(r)
            .iter()
            .map(|&i| (p[i].luxel_size.to_bits(), p[i].width, p[i].height))
            .collect();
        sizes.sort_unstable();
        sizes
    };
    assert_eq!(grid(&alone), grid(&with));
    assert_eq!(
        with.lightmap().finest_luxel(),
        alone.lightmap().finest_luxel()
    );
    let faces = with.lightmap().projections().len();
    let used = luxel_count(with.lightmap(), 0..faces);
    println!("{faces} faces, {used} luxels of {ATLAS_LUXELS}");
    assert!(faces > 1500, "{faces} faces");
    assert!(
        used as f64 / ATLAS_LUXELS < 0.5,
        "the room is far from the cap"
    );
}

#[test]
fn a_hundred_unit_lit_room_takes_a_sliver_of_the_atlas() {
    let mut brushes = vec![play_room(), vista_room(100.0, METAL)];
    brushes.extend(pillars(100.0));
    let corner = Vec3::new(AWAY as f32 - 45.0, -45.0, 8.0);
    let r = renderer(brushes, vec![play_lamp(), lamp(corner, 60.0), layer()]);
    let map = r.lightmap();
    let remote = |i: &usize| map.projections()[*i].origin.x > 500.0;
    let faces: Vec<usize> = (0..map.projections().len()).filter(remote).collect();
    let coarse = luxel_count(map, faces.iter().copied());
    // What the same faces would take at the play space's density.
    let polys: Vec<LightFace> = faces
        .iter()
        .map(|&i| LightFace::from(&r.scene().faces()[i]))
        .collect();
    let full: u64 = polys
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let p = project_face(f, i, config().luxel_size).expect("projects");
            u64::from(p.width) * u64::from(p.height)
        })
        .sum();
    let share = |n: u64| n as f64 / ATLAS_LUXELS * 100.0;
    println!(
        "{} faces: {coarse} luxels coarse ({:.3} %), {full} at full density ({:.1} %)",
        faces.len(),
        share(coarse),
        share(full)
    );
    assert!(share(coarse) < 5.0);
    assert!(
        share(coarse) * 20.0 < share(full),
        "coarsening is what saves it"
    );
}

#[test]
fn a_self_lit_room_adds_no_atlas_area() {
    let alone = renderer(vec![play_room()], vec![play_lamp()]);
    let brushes = vec![play_room(), vista_room(400.0, FIRST_SKY)];
    let with = renderer(
        brushes,
        vec![play_lamp(), layer_at(Vec3::new(AWAY as f32, 0.0, 0.0))],
    );
    let (a, b) = (alone.lightmap().atlas(), with.lightmap().atlas());
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let sky: Vec<usize> = (0..with.scene().faces().len())
        .filter(|&i| with.scene().faces()[i].material == FIRST_SKY)
        .collect();
    assert_eq!(sky.len(), 6, "the room has its six faces");
    assert_eq!(luxel_count(with.lightmap(), sky.iter().copied()), 0);
    assert!(sky.iter().all(|&i| with.lightmap().face_grid(i).is_none()));
    let all = |r: &LevelRenderer| luxel_count(r.lightmap(), 0..r.lightmap().projections().len());
    assert_eq!(all(&alone), all(&with));
}

#[test]
fn a_room_that_holds_the_player_start_is_not_coarsened() {
    // The layer sits in the room the player starts in: a leak the editor warns of.
    let start = Entity::PlayerStart {
        pos: Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
    };
    let leak = layer_at(Vec3::new(0.0, 8.0, 0.0));
    let sizes = |r: &LevelRenderer| -> Vec<f32> {
        r.lightmap()
            .projections()
            .iter()
            .map(|p| p.luxel_size)
            .collect()
    };
    let plain = sizes(&renderer(vec![play_room()], Vec::new()));
    assert_eq!(
        sizes(&renderer(vec![play_room()], vec![start, leak])),
        plain
    );
    // With no start at all, that same room counts as a view.
    let view = sizes(&renderer(vec![play_room()], vec![leak]));
    assert!(
        view.iter().zip(&plain).all(|(v, p)| v > p),
        "{view:?} {plain:?}"
    );
}

/// Every play-space luxel and the picture of an actor, with a huge lamp in the
/// vista room or without it.
fn with_far_lamp(huge: bool) -> LevelRenderer {
    let brushes = vec![play_room(), vista_room(64.0, METAL)];
    let mut entities = vec![play_lamp(), layer()];
    if huge {
        entities.push(lamp(Vec3::new(AWAY as f32, 0.0, 8.0), 2000.0));
    }
    renderer(brushes, entities)
}

fn actor_picture(r: &LevelRenderer, actors: &[Actor]) -> Vec<u32> {
    let mut fb = Framebuffer::new(W, H).expect("fb");
    let view = View::from_angles(Vec3::new(0.0, -4.0, -6.0), 0.0, 0.0, 0.0);
    r.render(&mut fb, &view, Quality::Low, actors, None)
        .expect("renders");
    fb.pixels().to_vec()
}

#[test]
fn a_huge_lamp_in_a_vista_room_changes_no_play_space_luxel() {
    let (plain, lit) = (with_far_lamp(false), with_far_lamp(true));
    let faces = play_faces(&plain);
    assert_eq!(faces.len(), 6);
    for f in faces {
        assert_eq!(
            luxels_of(plain.lightmap(), f),
            luxels_of(lit.lightmap(), f),
            "face {f}"
        );
    }
}

#[test]
fn a_huge_lamp_in_a_vista_room_changes_no_pixel_of_an_actor() {
    let actor = Actor {
        kind: ActorKind::Enemy,
        pos: Vec3::new(0.0, 8.0, -10.0),
        yaw: 0.0,
    };
    let (plain, lit) = (with_far_lamp(false), with_far_lamp(true));
    let without_actor = actor_picture(&plain, &[]);
    let a = actor_picture(&plain, &[actor]);
    assert_ne!(a, without_actor, "the actor is on screen");
    assert_eq!(a, actor_picture(&lit, &[actor]));
}

#[test]
fn a_self_lit_face_draws_flat_whatever_lamps_there_are() {
    // The stars are a texture now, so the room is not one colour: what must hold is
    // that no lamp, near or far, changes a pixel of it.
    let brushes = vec![cuboid(
        vd(-16.0, -16.0, -16.0),
        vd(16.0, 48.0, 16.0),
        Op::Subtract,
        0,
        FIRST_SKY,
    )];
    let lamps = |extra: Vec<Entity>| {
        let mut all = vec![lamp(Vec3::new(0.0, 16.0, 8.0), 60.0)];
        all.extend(extra);
        all
    };
    let far = lamp(Vec3::new(0.0, 16.0, 0.0), 2000.0);
    let none = renderer(brushes.clone(), Vec::new());
    let some = renderer(brushes.clone(), lamps(Vec::new()));
    let more = renderer(brushes, lamps(vec![far]));
    let pixels = |r: &LevelRenderer| {
        let mut fb = Framebuffer::new(W, H).expect("fb");
        let view = View::from_angles(Vec3::new(0.0, 0.0, 0.0), 0.3, 0.1, 0.0);
        r.render(&mut fb, &view, Quality::Low, &[], None)
            .expect("renders");
        fb.pixels().to_vec()
    };
    let shot = pixels(&none);
    assert!(shot.windows(2).any(|w| w[0] != w[1]), "the starfield shows");
    assert_eq!(shot, pixels(&some));
    assert_eq!(shot, pixels(&more));
    let panel = LIGHTPANEL;
    assert_ne!(panel, FIRST_SKY, "the light panel keeps its own light");
}

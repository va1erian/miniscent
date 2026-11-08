//! The room table is held to the portal flood it replaced: every leaf and face
//! the flood reached, the table reaches too.

use super::flood_oracle::Flood;
use super::*;
use mscent_math::rng::Rng;
use mscent_world::{GenParams, Level, compile, extract_faces, generate_level, showcase_level};

use crate::material::MaterialTable;

fn scene_of(level: &Level) -> Scene {
    let brushes = level.csg_brushes();
    let bsp = compile(&brushes).expect("a tree");
    let faces = extract_faces(&bsp, &brushes);
    let portals = Portals::build(&bsp);
    Scene::new(bsp, faces, portals, MaterialTable::default())
}

fn levels() -> Vec<(&'static str, Level)> {
    let gen_params = |n| GenParams {
        cols: n,
        rows: n,
        ..GenParams::default()
    };
    vec![
        ("showcase", showcase_level()),
        ("gen3x3", generate_level(7, &gen_params(3))),
        ("gen5x5", generate_level(11, &gen_params(5))),
    ]
}

/// A point somewhere in the level's air, or `None` if the draw landed in rock.
fn air(scene: &Scene, rng: &mut Rng) -> Option<Vec3> {
    let faces = scene.faces();
    let face = &faces[rng.below(faces.len() as u32) as usize];
    let at = face.polygon.vertices()[0] + face.plane.normal * f64::from(rng.range_f32(0.5, 6.0));
    let p = at.to_f32();
    (scene.bsp().leaf_kind(scene.bsp().leaf_at_f32(p)) == LeafKind::Empty).then_some(p)
}

#[test]
fn the_room_table_reaches_everything_the_old_flood_did() {
    let mut rng = Rng::new(0x5eed);
    for (name, level) in levels() {
        let scene = scene_of(&level);
        let (mut flood, mut reach) = (Flood::default(), LightReach::default());
        let (mut checked, mut flood_faces, mut table_faces) = (0usize, 0usize, 0usize);
        for _ in 0..3000 {
            let Some(pos) = air(&scene, &mut rng) else {
                continue;
            };
            let radius = rng.range_f32(0.5, 64.0);
            scene.flood_reach(pos, radius, &mut flood);
            scene.light_reach(pos, radius, &mut reach);
            for leaf in 0..scene.bsp().leaves().len() {
                let id = LeafId(leaf as u32);
                assert!(
                    !flood.has_leaf(id) || reach.has_leaf(id),
                    "{name}: leaf {leaf} from {pos:?} radius {radius}"
                );
            }
            for face in 0..scene.faces().len() {
                let (old, new) = (flood.has_face(face), reach.has_face(face));
                assert!(
                    !old || new,
                    "{name}: face {face} from {pos:?} radius {radius}"
                );
                flood_faces += usize::from(old);
                table_faces += usize::from(new);
            }
            checked += 1;
        }
        assert!(checked > 1000, "{name}: only {checked} lights were in air");
        assert!(flood_faces > 0, "{name}: the flood reached nothing");
        println!("{name}: {checked} lights, faces lit: flood {flood_faces}, table {table_faces}");
    }
}

#[test]
fn a_light_in_rock_or_out_of_range_reaches_nothing() {
    let scene = scene_of(&showcase_level());
    let mut reach = LightReach::default();
    let inside = scene.faces()[0].polygon.vertices()[0].to_f32()
        - scene.faces()[0].plane.normal.to_f32() * 0.5;
    for (pos, radius) in [
        (inside, 10.0),
        (Vec3::new(1.0e9, 0.0, 0.0), 10.0),
        (Vec3::new(f32::NAN, 0.0, 0.0), 10.0),
    ] {
        scene.light_reach(pos, radius, &mut reach);
        assert!((0..scene.faces().len()).all(|f| !reach.has_face(f)));
        assert!((0..scene.bsp().leaves().len()).all(|l| !reach.has_leaf(LeafId(l as u32))));
    }
    assert!(!reach.has_face(usize::MAX) && !reach.has_set(u32::MAX) && !reach.has_room(u32::MAX));
}

use super::*;
use crate::texture::{Material, MaterialTable};
use mscent_world::{Brush, GenParams, Op, compile, extract_faces, generate};

const COLOURS: [u32; 5] = [
    0xffc0_8060,
    0xff60_c080,
    0xff80_60c0,
    0xffc0_c0c0,
    0xff40_4040,
];

fn scene() -> Scene {
    let brushes = generate(7, &GenParams::default());
    let bsp = compile(&brushes).expect("compiles");
    let faces = extract_faces(&bsp, &brushes);
    let portals = mscent_world::Portals::build(&bsp);
    Scene::new(bsp, faces, portals, table())
}

fn table() -> MaterialTable {
    let mut table = MaterialTable::new();
    for (i, c) in COLOURS.into_iter().enumerate() {
        table.add(Material {
            name: format!("m{i}"),
            gloss: 0,
            texture: Texture::solid(c),
        });
    }
    table
}

fn centre(face: &Face) -> Vec3 {
    let verts = face.polygon.vertices();
    let sum = verts.iter().fold(Vec3d::ZERO, |a, &b| a + b);
    (sum * (1.0 / verts.len() as f64)).to_f32()
}

#[test]
fn a_point_on_any_face_reports_that_faces_material_and_tint() {
    let scene = scene();
    assert!(!scene.faces().is_empty());
    for face in scene.faces() {
        let found = scene
            .surface_at(centre(face), face.plane.normal.to_f32())
            .expect("a face is there");
        assert_eq!(found.material, face.material);
        let colour = COLOURS.get(usize::from(face.material)).copied();
        assert_eq!(Some(found.tint), colour);
    }
}

#[test]
fn hostile_values_and_empty_space_find_nothing() {
    let scene = scene();
    let up = Vec3::new(0.0, 0.0, 1.0);
    assert_eq!(scene.surface_at(Vec3::new(f32::NAN, 0.0, 0.0), up), None);
    assert_eq!(
        scene.surface_at(Vec3::ZERO, Vec3::new(0.0, f32::INFINITY, 0.0)),
        None
    );
    let far = Vec3::new(1.0e30, 1.0e30, 1.0e30);
    assert_eq!(scene.surface_at(far, up), None);
    assert_eq!(scene.surface_at(Vec3::ZERO, Vec3::ZERO), None);
}

#[test]
fn the_mean_of_a_filtered_texture_matches_its_texels() {
    let px = vec![0xff00_0000, 0xffff_ffff, 0xffff_ffff, 0xff00_0000];
    let plain = Texture::new(2, 2, px).expect("a valid size");
    let mean = mean_tint(&plain);
    assert_eq!(mean, 0xff7f_7f7f);
    let filtered = mean_tint(&plain.filtered());
    let channel = (filtered >> 8) & 0xff;
    assert!((0x7e..=0x80).contains(&channel), "{filtered:#x}");
}

#[test]
fn coplanar_faces_in_one_leaf_are_told_apart_by_the_point() {
    let d = Vec3d::new;
    let cuboid = |a, b, op, order| Brush::cuboid(a, b, op, order).expect("a brush");
    let brushes = vec![
        cuboid(d(-9.0, -9.0, -9.0), d(9.0, 9.0, 9.0), Op::Subtract, 0),
        // Two slabs side by side, their tops on one plane in one open room.
        cuboid(d(-9.0, -9.0, -9.0), d(0.0, 9.0, -5.0), Op::Add, 1).with_material(1),
        cuboid(d(0.0, -9.0, -9.0), d(9.0, 9.0, -5.0), Op::Add, 2).with_material(2),
    ];
    let bsp = compile(&brushes).expect("compiles");
    let faces = extract_faces(&bsp, &brushes);
    let portals = mscent_world::Portals::build(&bsp);
    let scene = Scene::new(bsp, faces, portals, table());
    let up = Vec3::new(0.0, 0.0, 1.0);
    let at = |x| {
        scene
            .surface_at(Vec3::new(x, 0.0, -5.0), up)
            .map(|s| s.material)
    };
    assert_eq!(at(-4.0), Some(1));
    assert_eq!(at(4.0), Some(2));
}

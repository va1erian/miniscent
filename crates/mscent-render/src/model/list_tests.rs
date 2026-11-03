//! What a renderer that projects vertices itself reads from a [`ModelList`].

use mscent_math::rng::Rng;
use mscent_math::{Quat, Vec3};
use mscent_world::LeafId;
use mscent_world::model::{build, read};

use super::*;
use crate::camera::Camera;
use crate::material::{Material, MaterialTable};
use crate::mesh::MeshLighting;
use crate::texture::Texture;

const PROBE: &str = "miniscent-blueprint 1
name probe
class drone
scale 0.5
skin hull metal
skin lens glass glow
part hub cell=0.5
solid ngon -2 0 0  2 0 0  2 1.5  6 hull
add sphere 2 0 0 1 lens
";

fn table() -> (ModelTable, ModelId) {
    let mut materials = MaterialTable::new();
    for name in ["metal", "glass"] {
        materials.add(Material {
            name: name.into(),
            gloss: 0,
            texture: Texture::solid(0xff80_8080),
        });
    }
    let blueprint = read(PROBE.as_bytes()).expect("reads");
    let model = build(&blueprint).expect("builds");
    let mut table = ModelTable::new();
    let id = table.add(&blueprint, &model, &materials);
    (table, id)
}

fn listed(list: &mut ModelList, table: &ModelTable, instances: &[ModelInstance]) {
    let cam = Camera::look_at(
        Vec3::new(0.0, -8.0, 1.0),
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, 1.0),
        1.0,
        0.1,
        400.0,
        320,
        180,
    )
    .expect("a camera");
    let light = MeshLighting {
        ambient: 0.5,
        sun_dir: Vec3::new(0.3, -0.2, 0.9),
        sun: 0.5,
        points: Vec::new(),
    };
    list.prepare(table, instances, &cam, &light, &[], |_| LeafId(0));
}

#[test]
fn a_parts_matrix_puts_its_points_where_software_does() {
    let (table, id) = table();
    let mut rng = Rng::new(7);
    for _ in 0..50 {
        let axis = Vec3::new(
            rng.range_f32(-1.0, 1.0),
            rng.range_f32(-1.0, 1.0),
            rng.range_f32(-1.0, 1.0),
        );
        let Some(axis) = axis.normalized() else {
            continue;
        };
        let instance = ModelInstance {
            scale: rng.range_f32(0.3, 3.0),
            ..ModelInstance::new(
                id,
                Vec3::new(rng.range_f32(-2.0, 2.0), 0.0, 0.0),
                Quat::from_axis_angle(axis, rng.range_f32(-3.0, 3.0)),
            )
        };
        let mut list = ModelList::default();
        listed(&mut list, &table, &[instance]);
        let part = list.parts().first().expect("in view");
        let p = Vec3::new(1.5, -0.5, 2.0);
        let w = part.to_world().transform_point(p);
        let gap = (Vec3::new(w.x, w.y, w.z) - part.place.point(p)).length();
        assert!(gap < 1e-4, "{gap}");
    }
}

#[test]
fn an_unprojected_list_has_the_same_parts_and_light_and_no_screen() {
    let (table, id) = table();
    let instances = [ModelInstance::new(id, Vec3::ZERO, Quat::IDENTITY)];
    let (mut projected, mut bare) = (ModelList::default(), ModelList::unprojected());
    listed(&mut projected, &table, &instances);
    listed(&mut bare, &table, &instances);
    assert!(!projected.screen.is_empty());
    assert!(bare.screen.is_empty());
    assert_eq!(bare.parts().len(), projected.parts().len());
    assert_eq!(bare.light, projected.light);
    assert_eq!(bare.triangles, projected.triangles);
}

#[test]
fn a_glowing_vertex_is_at_full_light_and_others_carry_the_lists() {
    let (table, id) = table();
    let mut list = ModelList::unprojected();
    listed(
        &mut list,
        &table,
        &[ModelInstance::new(id, Vec3::ZERO, Quat::IDENTITY)],
    );
    let part = list.parts()[0];
    let level = table.level_of(&part).expect("a level");
    let lit = list.vertex(&part, level, 0, false).expect("a vertex");
    let glowing = list.vertex(&part, level, 0, true).expect("a vertex");
    assert_eq!(lit.light, list.light[0]);
    assert_eq!(glowing.light, list::GLOW_LIGHT);
    assert!(
        list.vertex(&part, level, level.vertices.len(), false)
            .is_none()
    );
}

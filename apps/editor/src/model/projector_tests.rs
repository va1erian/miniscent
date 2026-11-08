//! The projector as the editor places and edits it: whatever the panel's fields
//! are given, the entity stays one that saves and loads.

use mscent_world::Entity;
use mscent_world::format;

use super::entity::{EntityKind, params, set_param};

fn fresh() -> Entity {
    EntityKind::Projector.make([1.0, 2.0, 3.0])
}

#[test]
fn a_new_projector_is_valid_and_points_down() {
    let e = fresh();
    assert!(e.is_valid());
    let p = params(&e);
    let names: Vec<_> = p.iter().map(|(n, _)| *n).collect();
    assert_eq!(names[0], "Yaw (deg)");
    assert_eq!(p[1], ("Pitch (deg)", -90.0));
}

#[test]
fn no_field_value_makes_an_entity_that_will_not_save() {
    let wild = [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0e9,
        -90.0,
        -1.0,
        0.0,
        0.4,
        1.0,
        91.0,
        360.0,
        1.0e9,
    ];
    for field in 0..params(&fresh()).len() {
        for v in wild {
            let mut e = fresh();
            set_param(&mut e, field, v);
            assert!(e.is_valid(), "field {field} set to {v}: {e:?}");
            let mut level = mscent_world::Level::default();
            level.entities.push(e);
            assert_eq!(format::read(&format::write(&level)).unwrap(), level);
        }
    }
}

#[test]
fn the_inner_angle_never_passes_the_outer() {
    let mut e = fresh();
    // Fields 7 and 8 are the inner and outer angles, in degrees.
    set_param(&mut e, 8, 20.0);
    set_param(&mut e, 7, 50.0);
    let Entity::Projector { inner, outer, .. } = e else {
        unreachable!()
    };
    assert!(inner <= outer, "{inner} against {outer}");
    set_param(&mut e, 8, 5.0);
    let Entity::Projector { inner, outer, .. } = e else {
        unreachable!()
    };
    assert!(inner <= outer && outer > 0.0, "{inner} against {outer}");
}

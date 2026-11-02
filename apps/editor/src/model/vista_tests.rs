//! The vista entity's panel fields, New Vista, and painting a View material.

use mscent_math::Vec3d;
use mscent_world::Entity;
use mscent_world::entity::{MAX_ENTITY_COORD, MAX_VISTA_FOG, MAX_VISTA_SPIN};
use mscent_world::materials::{
    SKY_PLANET, SKY_STARS, VIEW_FIRST, VIEW_SLOTS, view_material, view_slot,
};
use mscent_world::{Op, format, vista_warnings};

use super::entity::{EntityKind, params, set_param};
use super::facetex::FaceEdit;
use super::pick::{FaceRef, Ray, pick_visible_face};
use super::state::{EditorState, Selection};

/// A play room, 16 by 16 by 8 with a player start, and a goal.
fn level_with_room() -> EditorState {
    let mut st = EditorState::default();
    st.add_box([0.0; 3], [16.0, 16.0, 8.0]).expect("room");
    st.place_entity(EntityKind::PlayerStart, [8.0, 8.0, 2.0]);
    st
}

fn layers(st: &EditorState) -> Vec<Entity> {
    let all = st.doc.entities().iter().map(|e| e.entity);
    all.filter(|e| matches!(e, Entity::VistaLayer { .. }))
        .collect()
}

fn selected_layer(st: &EditorState) -> Entity {
    st.selected_entity().expect("a layer is selected").entity
}

#[test]
fn a_placed_layer_has_the_defaults_and_the_levels_middle() {
    let mut st = level_with_room();
    st.place_entity(EntityKind::VistaLayer, [100.0, 8.0, 4.0]);
    let Entity::VistaLayer {
        pos,
        centre,
        slot,
        layer,
        parallax,
        spin,
        pan,
        fog_end,
        ..
    } = selected_layer(&st)
    else {
        panic!("not a layer");
    };
    assert_eq!((pos.x, slot, layer, parallax), (100.0, 0, 0, 0.0));
    assert_eq!(
        (spin.length(), pan, fog_end),
        (0.0, [0.0, 0.0], MAX_VISTA_FOG)
    );
    // The middle of the room and the start: x 0..16, y 0..16, z 0..8.
    assert_eq!((centre.x, centre.y, centre.z), (8.0, 8.0, 4.0));
}

#[test]
fn no_panel_edit_makes_a_layer_that_will_not_save() {
    let mut st = level_with_room();
    st.place_entity(EntityKind::VistaLayer, [100.0, 8.0, 4.0]);
    let count = params(&selected_layer(&st)).len();
    assert_eq!(count, 16, "every field is in the panel");
    let junk = [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0e30,
        1.0e30,
        -5.0,
        300.0,
        0.5,
        99_999.5,
    ];
    for i in 0..count + 2 {
        for v in junk {
            let mut e = selected_layer(&st);
            set_param(&mut e, i, v);
            assert!(e.is_valid(), "field {i} set to {v} left {e:?}");
        }
    }
    // Spot checks of the ranges.
    let mut e = selected_layer(&st);
    set_param(&mut e, 0, 99.0);
    set_param(&mut e, 1, 99.0);
    set_param(&mut e, 2, 3.0);
    set_param(&mut e, 4, -1.0e30);
    set_param(&mut e, 8, 1.0e30);
    let Entity::VistaLayer {
        slot,
        layer,
        parallax,
        centre,
        spin,
        ..
    } = e
    else {
        panic!("not a layer");
    };
    assert_eq!((slot, layer, parallax), (VIEW_SLOTS - 1, 3, 1.0));
    assert_eq!((centre.y, spin.z), (-MAX_ENTITY_COORD, MAX_VISTA_SPIN));
}

#[test]
fn fog_start_and_end_push_each_other_and_the_colour_has_three_channels() {
    let mut st = level_with_room();
    st.place_entity(EntityKind::VistaLayer, [100.0, 8.0, 4.0]);
    let mut e = selected_layer(&st);
    set_param(&mut e, 12, 50.0);
    set_param(&mut e, 11, 80.0);
    let Entity::VistaLayer {
        fog_start, fog_end, ..
    } = e
    else {
        panic!("not a layer");
    };
    assert_eq!((fog_start, fog_end), (80.0, 81.0));
    set_param(&mut e, 12, 10.0);
    let Entity::VistaLayer { fog_end, .. } = e else {
        panic!("not a layer");
    };
    assert_eq!(fog_end, 81.0, "the end stays above the start");
    for (i, v) in [(13, 0x12), (14, 0x34), (15, 0x56)] {
        set_param(&mut e, i, f64::from(v));
    }
    let Entity::VistaLayer { fog_color, .. } = e else {
        panic!("not a layer");
    };
    assert_eq!(fog_color, 0x123456);
    assert_eq!(
        params(&e)[13..],
        [("Fog red", 18.0), ("Fog green", 52.0), ("Fog blue", 86.0)]
    );
}

#[test]
fn every_layer_edit_is_one_undo_step_and_undoes() {
    let mut st = level_with_room();
    st.place_entity(EntityKind::VistaLayer, [100.0, 8.0, 4.0]);
    let before = st.doc.clone();
    for i in 0..16 {
        let depth = st.history.depth();
        let edited = st.edit_selected_entity(|e| set_param(e, i, 2.0));
        assert_eq!(st.history.depth(), depth + usize::from(edited), "field {i}");
        if edited {
            st.undo();
            assert_eq!(st.history.depth(), depth);
        }
        assert_eq!(st.doc, before, "field {i}");
    }
}

#[test]
fn new_vista_builds_the_recipe_in_one_undo_step() {
    let mut st = level_with_room();
    let before = st.doc.clone();
    let depth = st.history.depth();
    assert!(st.new_vista(), "{}", st.message);
    assert_eq!(st.history.depth(), depth + 1);
    assert_eq!(st.doc.brushes().len(), before.brushes().len() + 2);
    let Entity::VistaLayer {
        pos,
        centre,
        slot,
        layer,
        parallax,
        spin,
        fog_end,
        ..
    } = selected_layer(&st)
    else {
        panic!("the new layer is selected");
    };
    assert_eq!((slot, layer, parallax, fog_end), (0, 0, 0.0, MAX_VISTA_FOG));
    assert!(spin.length() > 0.0 && spin.length() < 0.2, "a slow spin");
    assert_eq!((centre.x, centre.y, centre.z), (8.0, 8.0, 4.0));
    // Beyond the level with a clear margin on every side.
    let room = st.doc.brushes().iter().rev().nth(1).expect("the room");
    let (lo, hi) = room.bounds();
    assert_eq!(room.op, Op::Subtract);
    assert!(
        lo[0] >= 16.0 + 8.0 || hi[0] <= -8.0,
        "clear of the level in x"
    );
    assert!((0..3).all(|i| hi[i] - lo[i] == 64.0));
    assert!(
        pos.x > lo[0] as f32 && pos.x < hi[0] as f32,
        "the layer is in the room"
    );
    assert!(st.doc.to_level().is_ok());
    st.undo();
    // The id counter is not undone, as with every command.
    assert_eq!(st.doc.brushes(), before.brushes());
    assert_eq!(
        st.doc.entities(),
        before.entities(),
        "one undo removes all of it"
    );
    st.redo();
    assert_eq!(layers(&st).len(), 1);
}

#[test]
fn new_vista_faces_have_the_materials_and_a_fitted_planet() {
    let mut st = level_with_room();
    st.texture_sizes = vec![(128, 128); 14];
    st.texture_sizes[usize::from(SKY_PLANET)] = (256, 256);
    st.texture_sizes[usize::from(SKY_STARS)] = (512, 512);
    assert!(st.new_vista());
    let brushes = st.doc.brushes();
    let (room, plate) = (&brushes[brushes.len() - 2], &brushes[brushes.len() - 1]);
    assert!(room.full_texinfos().iter().all(|t| t.material == SKY_STARS));
    assert!(room.full_texinfos().iter().all(|t| t.u_scale == 0.125));
    let planets: Vec<_> = plate
        .full_texinfos()
        .into_iter()
        .filter(|t| t.material == SKY_PLANET)
        .collect();
    assert_eq!(planets.len(), 1, "the disc is on one face");
    // 24 units over a 256 texel texture: it covers the face once.
    assert!((planets[0].u_scale - 24.0 / 256.0).abs() < 1.0e-6);
    assert!((planets[0].v_scale - 24.0 / 256.0).abs() < 1.0e-6);
}

#[test]
fn a_new_vista_is_a_clean_level_in_every_way_the_game_checks() {
    let mut st = level_with_room();
    assert!(st.new_vista());
    let level = st.doc.to_level().expect("exports");
    assert!(level.entities.iter().all(|e| e.is_valid()));
    let back = format::read(&format::write(&level)).expect("saves and loads");
    assert_eq!(back.entities, level.entities);
    assert_eq!(
        vista_warnings(&level),
        [],
        "sealed, and not in the play space"
    );
}

#[test]
fn each_new_vista_takes_the_next_slot_and_the_ninth_does_nothing() {
    let mut st = level_with_room();
    for want in 0..VIEW_SLOTS {
        assert_eq!(st.free_vista_slot(), Some(want));
        assert!(st.new_vista(), "{}", st.message);
        let Entity::VistaLayer { slot, centre, .. } = selected_layer(&st) else {
            panic!("not a layer");
        };
        assert_eq!(slot, want);
        assert_eq!((centre.x, centre.y), (8.0, 8.0), "all agree on the centre");
    }
    let after = st.doc.clone();
    let depth = st.history.depth();
    assert!(!st.new_vista());
    assert_eq!((st.doc.clone(), st.history.depth()), (after, depth));
    assert!(st.message.contains("slots"), "{}", st.message);
    let level = st.doc.to_level().expect("exports");
    assert_eq!(vista_warnings(&level), [], "the rooms stay sealed apart");
    assert!(format::read(&format::write(&level)).is_ok());
}

#[test]
fn an_empty_level_gets_a_vista_too() {
    let mut st = EditorState::default();
    assert!(st.new_vista());
    assert!(st.doc.to_level().is_ok());
}

/// A ray into the room's +y wall from its middle.
fn wall_ray() -> Ray {
    Ray {
        origin: Vec3d::new(8.0, 8.0, 4.0),
        dir: Vec3d::new(0.0, 1.0, 0.0),
    }
}

#[test]
fn a_wall_painted_with_a_view_can_still_be_clicked_and_painted_back() {
    let mut st = level_with_room();
    assert!(st.new_vista());
    let room = st.doc.brushes()[0].id;
    let wall = pick_visible_face(&st.doc, &wall_ray()).expect("the wall");
    let original = st.face_texinfo(wall.face).expect("mapping");
    st.selection = Some(Selection::Brush(room));
    st.faces = vec![wall.face];
    let view = view_material(0).expect("slot 0");
    assert!(st.apply_face_edit(FaceEdit::Material(view)));
    assert_eq!(
        st.face_texinfo(wall.face).map(|t| view_slot(t.material)),
        Some(Some(0))
    );
    // The same click finds the same face: picking is of the brushes, not of the
    // renderer's faces, which hold no viewport face.
    let again = pick_visible_face(&st.doc, &wall_ray()).expect("still clickable");
    assert_eq!(again.face, wall.face);
    let hit = st.pick_face_at(&wall_ray(), false).expect("picked");
    assert_eq!((hit.face, st.faces.clone()), (wall.face, vec![wall.face]));
    assert!(st.apply_face_edit(FaceEdit::Material(original.material)));
    assert_eq!(
        st.face_texinfo(wall.face).map(|t| t.material),
        Some(original.material)
    );
    let FaceRef { brush, .. } = wall.face;
    assert_eq!(st.doc.brush(brush).map(|b| b.material), Some(0));
}

#[test]
fn view_ids_are_safe_in_texture_size_lookups() {
    let st = EditorState {
        texture_sizes: vec![(64, 64); 20],
        ..EditorState::default()
    };
    for id in [VIEW_FIRST, VIEW_FIRST + 7, u16::MAX] {
        assert_eq!(st.texture_size(id), super::facetex::DEFAULT_TEXTURE);
    }
}

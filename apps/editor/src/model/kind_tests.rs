//! Enemy kinds through the editor (M19.1): a level's kinds survive opening and
//! saving, and no field value gives an enemy a kind its level lacks.

use mscent_math::Vec3;
use mscent_world::kinds::stock_kinds;
use mscent_world::{Entity, Level, format};

use super::Document;
use super::entity::{EntityKind, clamp_kind, set_param};

fn enemy(kind: u8) -> Entity {
    Entity::Enemy {
        pos: Vec3::new(1.0, 2.0, 3.0),
        kind,
    }
}

#[test]
fn kinds_survive_opening_and_saving() {
    let mut kinds = stock_kinds();
    kinds.push(kinds[1].clone());
    kinds[2].name = "hunter-2".to_string();
    let level = Level {
        entities: vec![enemy(2)],
        kinds,
        ..Level::default()
    };
    let doc = Document::from_level(&level);
    assert_eq!(doc.kind_count(), 3);
    let saved = doc.to_level().unwrap();
    assert_eq!(saved.kinds, level.kinds);
    assert_eq!(
        format::read(&format::write(&saved)).unwrap().kinds,
        level.kinds
    );
}

#[test]
fn a_level_with_no_kinds_offers_the_stock_two() {
    let doc = Document::from_level(&Level::default());
    assert!(doc.kinds().is_empty());
    assert_eq!(doc.kind_count(), 2);
}

#[test]
fn no_kind_field_value_names_a_kind_past_the_table() {
    let cases = [
        (2, 255.0, 1, true),
        (2, 1.0, 1, false),
        (5, 9.0, 4, true),
        (5, -3.0, 0, false),
        (32, 255.0, 31, false),
    ];
    for (count, v, want, told) in cases {
        let mut e = EntityKind::Enemy.make([0.0; 3]);
        set_param(&mut e, 0, v);
        let note = clamp_kind(&mut e, count);
        let kind = want;
        assert_eq!(
            e,
            Entity::Enemy {
                pos: Vec3::ZERO,
                kind
            },
            "{count} {v}"
        );
        assert_eq!(note.is_some(), told, "{count} {v}: {note:?}");
    }
    let mut e = EntityKind::Enemy.make([0.0; 3]);
    set_param(&mut e, 0, 9.0);
    assert_eq!(
        clamp_kind(&mut e, 2).as_deref(),
        Some("Kind must be 0 to 1: set to 1")
    );
}

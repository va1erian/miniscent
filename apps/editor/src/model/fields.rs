//! The range and step of each entity field the panel shows, in the order of
//! [`super::entity::params`]. One policy for every field: a value past its range
//! is clamped to it, and the status bar says so, naming the field.

use mscent_world::Entity;
use mscent_world::entity::{
    MAX_ENTITY_COORD, MAX_FOG_DEPTH, MAX_SHIP_SCALE, MAX_VISTA_FOG, MAX_VISTA_LAYERS,
    MAX_VISTA_PAN, MAX_VISTA_SPIN, MIN_SHIP_SCALE,
};
use mscent_world::materials::VIEW_SLOTS;

/// Largest value a field of any entity but a vista layer takes.
pub const MAX_PLAIN_PARAM: f64 = 4096.0;

/// Least radius or half size: a zero one would hold nothing.
const LEAST_SIZE: f64 = 0.1;

/// The most enemy kinds a level holds; the level's own table holds a Kind
/// tighter (`super::entity::clamp_kind`).
const ENEMY_KINDS: f64 = (mscent_world::kinds::MAX_LEVEL_KINDS - 1) as f64;

/// The values a field takes and how far its steppers move it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limit {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

const fn limit(min: f64, max: f64, step: f64) -> Limit {
    Limit { min, max, step }
}

const UNIT: Limit = limit(0.0, 1.0, 0.05);
const YAW: Limit = limit(-360.0, 360.0, 15.0);
const SIZE: Limit = limit(LEAST_SIZE, MAX_PLAIN_PARAM, 0.5);
const INTENSITY: Limit = limit(0.0, MAX_PLAIN_PARAM, 0.1);
const BYTE: Limit = limit(0.0, 255.0, 1.0);

/// The limits of `e`'s fields, one per entry of `params(e)`.
pub fn limits(e: &Entity) -> Vec<Limit> {
    match e {
        Entity::Light { .. } => vec![UNIT, UNIT, UNIT, SIZE, INTENSITY],
        Entity::Projector { .. } => vec![
            YAW,
            limit(-90.0, 90.0, 5.0),
            UNIT,
            UNIT,
            UNIT,
            SIZE,
            INTENSITY,
            limit(0.0, 90.0, 1.0),
            limit(0.5, 90.0, 1.0),
        ],
        Entity::Goal { .. } => vec![SIZE],
        Entity::Scale { .. } => vec![limit(
            f64::from(MIN_SHIP_SCALE),
            f64::from(MAX_SHIP_SCALE),
            0.25,
        )],
        Entity::World { .. } => {
            let depth = f64::from(MAX_FOG_DEPTH);
            vec![limit(0.0, depth - 1.0, 10.0), limit(1.0, depth, 10.0)]
        }
        Entity::Enemy { .. } => vec![limit(0.0, ENEMY_KINDS, 1.0)],
        Entity::PlayerStart { .. } | Entity::Marker { .. } => vec![YAW],
        Entity::Zone { .. } => vec![SIZE; 3],
        Entity::VistaLayer { .. } => vista_limits(),
    }
}

fn vista_limits() -> Vec<Limit> {
    let coord = f64::from(MAX_ENTITY_COORD);
    let spin = f64::from(MAX_VISTA_SPIN);
    let pan = f64::from(MAX_VISTA_PAN);
    let fog = f64::from(MAX_VISTA_FOG);
    let mut out = vec![
        limit(0.0, f64::from(VIEW_SLOTS - 1), 1.0),
        limit(0.0, f64::from(MAX_VISTA_LAYERS - 1), 1.0),
        limit(0.0, MAX_PLAIN_PARAM, 1.0),
    ];
    out.extend([limit(-coord, coord, 1.0); 3]);
    out.extend([limit(-spin, spin, 0.1); 3]);
    out.extend([limit(-pan, pan, 1.0); 2]);
    out.extend([limit(0.0, fog - 1.0, 10.0), limit(1.0, fog, 10.0)]);
    out.extend([BYTE; 3]);
    out
}

/// `v` held to `limit`, and what to tell the user when it had to be: the
/// field's name, its range and what it was set to. `None` for not a number.
pub fn held(label: &str, limit: Limit, v: f64) -> Option<(f64, Option<String>)> {
    if !v.is_finite() {
        return None;
    }
    let kept = v.clamp(limit.min, limit.max);
    let note = (kept != v).then(|| {
        format!(
            "{} must be {} to {}: set to {kept}",
            plain(label),
            limit.min,
            limit.max
        )
    });
    Some((kept, note))
}

/// The label without its unit: "Yaw (deg)" is "Yaw".
fn plain(label: &str) -> &str {
    label.split(" (").next().unwrap_or(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entity::{EntityKind, params};

    #[test]
    fn every_field_has_a_limit_and_its_value_is_within_it() {
        for kind in [
            EntityKind::PlayerStart,
            EntityKind::Light,
            EntityKind::Projector,
            EntityKind::Enemy,
            EntityKind::Goal,
            EntityKind::Marker,
            EntityKind::Zone,
            EntityKind::VistaLayer,
        ] {
            let e = kind.make([0.0; 3]);
            let (shown, range) = (params(&e), limits(&e));
            assert_eq!(shown.len(), range.len(), "{kind:?}");
            for ((label, v), l) in shown.iter().zip(range) {
                assert!(l.min <= *v && *v <= l.max, "{kind:?} {label} {v}");
                assert!(l.step > 0.0 && l.step <= l.max - l.min, "{kind:?} {label}");
            }
        }
    }

    #[test]
    fn a_clamp_names_the_field_and_its_range() {
        let (v, note) = held("Red", UNIT, 2.0).expect("a number");
        assert_eq!(v, 1.0);
        assert_eq!(note.as_deref(), Some("Red must be 0 to 1: set to 1"));
        assert_eq!(held("Red", UNIT, 0.5), Some((0.5, None)));
        assert_eq!(held("Red", UNIT, f64::NAN), None);
        let (_, note) = held("Yaw (deg)", YAW, 400.0).expect("a number");
        assert!(note.unwrap().starts_with("Yaw must be -360 to 360"));
    }
}

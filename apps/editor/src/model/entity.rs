//! Entity kinds, defaults and position access.

use mscent_math::Vec3;
use mscent_world::Entity;
use mscent_world::entity::{
    MAX_ENTITY_COORD, MAX_VISTA_FOG, MAX_VISTA_LAYERS, MAX_VISTA_PAN, MAX_VISTA_SPIN,
};
use mscent_world::materials::VIEW_SLOTS;
use mscent_world::vista_scale::{parallax_of_scale, scale_of_parallax};

/// Largest value a field of any entity but a vista layer takes.
const MAX_PLAIN_PARAM: f64 = 4096.0;

/// What the entity tools can place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    PlayerStart,
    Light,
    Enemy,
    Goal,
    Marker,
    Zone,
    VistaLayer,
}

impl EntityKind {
    pub fn name(self) -> &'static str {
        match self {
            EntityKind::PlayerStart => "Player start",
            EntityKind::Light => "Light",
            EntityKind::Enemy => "Enemy",
            EntityKind::Goal => "Goal",
            EntityKind::Marker => "Marker",
            EntityKind::Zone => "Zone",
            EntityKind::VistaLayer => "Vista Layer",
        }
    }

    /// A new entity of this kind at `p` with default parameters.
    pub fn make(self, p: [f64; 3]) -> Entity {
        let pos = Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
        match self {
            EntityKind::PlayerStart => Entity::PlayerStart { pos, yaw: 0.0 },
            EntityKind::Light => Entity::Light {
                pos,
                color: Vec3::new(1.0, 0.9, 0.7),
                radius: 16.0,
                intensity: 1.2,
            },
            EntityKind::Enemy => Entity::Enemy { pos, kind: 0 },
            EntityKind::Goal => Entity::Goal { pos, radius: 1.5 },
            EntityKind::Marker => Entity::Marker { pos, yaw: 0.0 },
            EntityKind::Zone => Entity::Zone {
                pos,
                half: Vec3::new(2.0, 2.0, 2.0),
            },
            // No fog, no motion, and the play space's centre is the caller's to
            // set: this does not know the level.
            EntityKind::VistaLayer => Entity::VistaLayer {
                pos,
                centre: pos,
                slot: 0,
                layer: 0,
                parallax: 0.0,
                spin: Vec3::new(0.0, 0.0, 0.0),
                pan: [0.0, 0.0],
                fog_start: 0.0,
                fog_end: MAX_VISTA_FOG,
                fog_color: 0,
            },
        }
    }
}

/// The world entity for a fog range, at the origin.
pub fn fog_entity(fog_start: f32, fog_end: f32) -> Entity {
    Entity::World {
        pos: Vec3::new(0.0, 0.0, 0.0),
        fog_start,
        fog_end,
    }
}

/// What the properties panel calls `e`. The world entity is not placeable:
/// the level settings dialog creates it.
pub fn entity_name(e: &Entity) -> &'static str {
    match e {
        Entity::PlayerStart { .. } => EntityKind::PlayerStart.name(),
        Entity::Light { .. } => EntityKind::Light.name(),
        Entity::Enemy { .. } => EntityKind::Enemy.name(),
        Entity::Goal { .. } => EntityKind::Goal.name(),
        Entity::World { .. } => "World",
        Entity::Marker { .. } => EntityKind::Marker.name(),
        Entity::Zone { .. } => EntityKind::Zone.name(),
        Entity::VistaLayer { .. } => EntityKind::VistaLayer.name(),
    }
}

/// An `f32` shown as a short decimal (no `0.8500000238418579`).
fn short(v: f32) -> f64 {
    (v as f64 * 1.0e4).round() / 1.0e4
}

pub fn pos(e: &Entity) -> [f64; 3] {
    let p = e.pos();
    [short(p.x), short(p.y), short(p.z)]
}

/// The entity moved to `p`.
pub fn with_pos(e: &Entity, p: [f64; 3]) -> Entity {
    let np = Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32);
    let mut out = *e;
    match &mut out {
        Entity::PlayerStart { pos, .. }
        | Entity::Light { pos, .. }
        | Entity::Enemy { pos, .. }
        | Entity::Goal { pos, .. }
        | Entity::World { pos, .. }
        | Entity::Marker { pos, .. }
        | Entity::Zone { pos, .. }
        | Entity::VistaLayer { pos, .. } => *pos = np,
    }
    out
}

/// The kind-specific numeric fields as `(label, value)`, in panel order.
pub fn params(e: &Entity) -> Vec<(&'static str, f64)> {
    match *e {
        Entity::Light {
            color,
            radius,
            intensity,
            ..
        } => vec![
            ("Red", short(color.x)),
            ("Green", short(color.y)),
            ("Blue", short(color.z)),
            ("Radius", short(radius)),
            ("Intensity", short(intensity)),
        ],
        Entity::Goal { radius, .. } => vec![("Radius", short(radius))],
        Entity::World {
            fog_start, fog_end, ..
        } => vec![("Fog start", short(fog_start)), ("Fog end", short(fog_end))],
        Entity::Enemy { kind, .. } => vec![("Kind", kind as f64)],
        Entity::PlayerStart { yaw, .. } | Entity::Marker { yaw, .. } => {
            vec![("Yaw (deg)", short(yaw.to_degrees()))]
        }
        Entity::Zone { half, .. } => vec![
            ("Half X", short(half.x)),
            ("Half Y", short(half.y)),
            ("Half Z", short(half.z)),
        ],
        Entity::VistaLayer {
            centre,
            slot,
            layer,
            parallax,
            spin,
            pan,
            fog_start,
            fog_end,
            fog_color,
            ..
        } => vec![
            ("Slot", f64::from(slot)),
            ("Layer", f64::from(layer)),
            ("Scale (0 = infinite)", short(scale_of_parallax(parallax))),
            ("Centre X", short(centre.x)),
            ("Centre Y", short(centre.y)),
            ("Centre Z", short(centre.z)),
            ("Spin X", short(spin.x)),
            ("Spin Y", short(spin.y)),
            ("Spin Z", short(spin.z)),
            ("Pan U", short(pan[0])),
            ("Pan V", short(pan[1])),
            ("Fog start", short(fog_start)),
            ("Fog end", short(fog_end)),
            ("Fog red", f64::from((fog_color >> 16) & 0xff)),
            ("Fog green", f64::from((fog_color >> 8) & 0xff)),
            ("Fog blue", f64::from(fog_color & 0xff)),
        ],
    }
}

/// Sets field `i` of [`params`]; out-of-range indices change nothing.
pub fn set_param(e: &mut Entity, i: usize, v: f64) {
    // The panel's fields are wide for a vista layer's fog, which holds its own limits.
    let v = if matches!(e, Entity::VistaLayer { .. }) {
        v
    } else {
        v.clamp(-MAX_PLAIN_PARAM, MAX_PLAIN_PARAM)
    };
    match e {
        Entity::Light {
            color,
            radius,
            intensity,
            ..
        } => match i {
            0 => color.x = v as f32,
            1 => color.y = v as f32,
            2 => color.z = v as f32,
            3 => *radius = v as f32,
            4 => *intensity = v as f32,
            _ => {}
        },
        Entity::Goal { radius, .. } if i == 0 => *radius = v as f32,
        Entity::World { fog_start, .. } if i == 0 => *fog_start = v as f32,
        Entity::World { fog_end, .. } if i == 1 => *fog_end = v as f32,
        Entity::Enemy { kind, .. } if i == 0 => *kind = v.clamp(0.0, 255.0) as u8,
        Entity::PlayerStart { yaw, .. } | Entity::Marker { yaw, .. } if i == 0 => {
            *yaw = (v as f32).to_radians()
        }
        Entity::Zone { half, .. } => match i {
            0 => half.x = v as f32,
            1 => half.y = v as f32,
            2 => half.z = v as f32,
            _ => {}
        },
        Entity::VistaLayer { .. } => set_vista_param(e, i, v),
        _ => {}
    }
}

/// Field `i` of a vista layer, held to the ranges of `Entity::is_valid` so no
/// edit can make a level that will not save or load. A value that is not a
/// number changes nothing.
fn set_vista_param(e: &mut Entity, i: usize, v: f64) {
    let Entity::VistaLayer {
        centre,
        slot,
        layer,
        parallax,
        spin,
        pan,
        fog_start,
        fog_end,
        fog_color,
        ..
    } = e
    else {
        return;
    };
    if !v.is_finite() {
        return;
    }
    let within = |limit: f32| v.clamp(-f64::from(limit), f64::from(limit)) as f32;
    match i {
        0 => *slot = whole(v, VIEW_SLOTS),
        1 => *layer = whole(v, MAX_VISTA_LAYERS),
        // Scale is how small the room is built: 1 to 4096, or 0 for infinitely far.
        2 => *parallax = parallax_of_scale(v as f32),
        3 => centre.x = within(MAX_ENTITY_COORD),
        4 => centre.y = within(MAX_ENTITY_COORD),
        5 => centre.z = within(MAX_ENTITY_COORD),
        6 => spin.x = within(MAX_VISTA_SPIN),
        7 => spin.y = within(MAX_VISTA_SPIN),
        8 => spin.z = within(MAX_VISTA_SPIN),
        9 => pan[0] = within(MAX_VISTA_PAN),
        10 => pan[1] = within(MAX_VISTA_PAN),
        // The end must stay above the start, so each pushes the other.
        11 => {
            *fog_start = v.clamp(0.0, f64::from(MAX_VISTA_FOG - 1.0)) as f32;
            *fog_end = fog_end.max((*fog_start + 1.0).min(MAX_VISTA_FOG));
        }
        12 => {
            let least = (*fog_start + 1.0).min(MAX_VISTA_FOG);
            *fog_end = v.clamp(f64::from(least), f64::from(MAX_VISTA_FOG)) as f32;
        }
        13..=15 => {
            let shift = (15 - i as u32) * 8;
            let channel = v.round().clamp(0.0, 255.0) as u32;
            *fog_color = (*fog_color & !(0xff << shift)) | (channel << shift);
        }
        _ => {}
    }
}

/// `v` as a whole number below `limit`; the panel can type anything.
fn whole(v: f64, limit: u8) -> u8 {
    v.clamp(0.0, f64::from(limit - 1)) as u8
}

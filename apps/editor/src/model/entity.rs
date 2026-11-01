//! Entity kinds, defaults and position access.

use mscent_math::Vec3;
use mscent_world::Entity;

/// What the entity tools can place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    PlayerStart,
    Light,
    Enemy,
    Goal,
    World,
    Marker,
    Zone,
}

impl EntityKind {
    pub fn name(self) -> &'static str {
        match self {
            EntityKind::PlayerStart => "Player start",
            EntityKind::Light => "Light",
            EntityKind::Enemy => "Enemy",
            EntityKind::Goal => "Goal",
            EntityKind::World => "World",
            EntityKind::Marker => "Marker",
            EntityKind::Zone => "Zone",
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
            EntityKind::World => Entity::World {
                pos,
                fog_start: 4.0,
                fog_end: 26.0,
            },
            EntityKind::Marker => Entity::Marker { pos, yaw: 0.0 },
            EntityKind::Zone => Entity::Zone {
                pos,
                half: Vec3::new(2.0, 2.0, 2.0),
            },
        }
    }
}

pub fn kind(e: &Entity) -> EntityKind {
    match e {
        Entity::PlayerStart { .. } => EntityKind::PlayerStart,
        Entity::Light { .. } => EntityKind::Light,
        Entity::Enemy { .. } => EntityKind::Enemy,
        Entity::Goal { .. } => EntityKind::Goal,
        Entity::World { .. } => EntityKind::World,
        Entity::Marker { .. } => EntityKind::Marker,
        Entity::Zone { .. } => EntityKind::Zone,
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
        | Entity::Zone { pos, .. } => *pos = np,
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
    }
}

/// Sets field `i` of [`params`]; out-of-range indices change nothing.
pub fn set_param(e: &mut Entity, i: usize, v: f64) {
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
        _ => {}
    }
}

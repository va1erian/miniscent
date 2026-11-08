//! Game entities placed in a level (M2.6, reused by M6.2).
//!
//! Positions are `f32` world units: entities are runtime data, not CSG input.
//! [`Entity::is_valid`] is what the level reader enforces, so a loaded level
//! never carries NaN, infinite or non-positive-radius values.

use crate::brush::Brush;
use crate::labels::{EntityLabel, MoverGroup};
use crate::materials::VIEW_SLOTS;
use crate::voxel::Volume;
use mscent_math::Vec3;

/// Largest absolute coordinate, radius or intensity accepted for an entity.
pub const MAX_ENTITY_COORD: f32 = 1.0e6;

/// One placed game element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Entity {
    /// Where the player spawns, facing `yaw` radians.
    PlayerStart { pos: Vec3, yaw: f32 },
    /// Point light; `color` is linear RGB in `0..=1` per channel.
    Light {
        pos: Vec3,
        color: Vec3,
        radius: f32,
        intensity: f32,
    },
    /// A spot light that shines one way, a projector. It lights as a point light
    /// of the same `color`, `radius` and `intensity` does, but only inside a
    /// cone about the direction given by `yaw` (radians about +Z from +X) and
    /// `pitch` (radians up from the horizon, within a quarter turn either way).
    /// The light is full within `inner` of the axis, fades to nothing at `outer`,
    /// and both are half angles in radians, `0 <= inner <= outer <= PI / 2`.
    Projector {
        pos: Vec3,
        yaw: f32,
        pitch: f32,
        color: Vec3,
        radius: f32,
        intensity: f32,
        inner: f32,
        outer: f32,
    },
    /// Enemy spawn; `kind` indexes the game's enemy table.
    Enemy { pos: Vec3, kind: u8 },
    /// Reaching within `radius` of `pos` wins the level.
    Goal { pos: Vec3, radius: f32 },
    /// Per-level look settings; the first one in a level counts. `pos` is
    /// only where the editor draws it. Fog starts at view depth `fog_start`
    /// and is total at `fog_end`.
    World {
        pos: Vec3,
        fog_start: f32,
        fog_end: f32,
    },
    /// A point a script refers to by name: where to spawn something, a hinge.
    Marker { pos: Vec3, yaw: f32 },
    /// A box a script is told the player entered or left. `half` is the
    /// half-size on each axis.
    Zone { pos: Vec3, half: Vec3 },
    /// One layer of a viewport vista, placed in a sealed room. The layer's eye is
    /// `pos + (player - centre) * parallax`, turned by `spin` over time.
    /// `centre` is the point of the play space that maps to `pos`. `slot` picks
    /// the vista (the viewport material `VIEW_FIRST + slot`) and `layer` the order
    /// inside it, 0 the farthest. `parallax` in `0..=1` is how far the eye moves per
    /// unit the player moves: 0 is infinitely far, 1/16 a miniature seen 16 times
    /// its size. `spin` is an axis times radians per second. `pan` is texels per
    /// second, on self-lit faces only. Fog runs from `fog_start` to `fog_end` in
    /// `fog_color` (0xRRGGBB); `0` to [`MAX_VISTA_FOG`] is effectively no fog.
    VistaLayer {
        pos: Vec3,
        centre: Vec3,
        slot: u8,
        layer: u8,
        parallax: f32,
        spin: Vec3,
        pan: [f32; 2],
        fog_start: f32,
        fog_end: f32,
        fog_color: u32,
    },
}

/// Layers in one vista, so `layer` is below this.
pub const MAX_VISTA_LAYERS: u8 = 4;
/// Largest fog distance a vista layer may ask for. A layer is a miniature, so its
/// own units reach farther than the play space's [`MAX_FOG_DEPTH`].
pub const MAX_VISTA_FOG: f32 = 100_000.0;
/// Largest `spin` component, in radians per second.
pub const MAX_VISTA_SPIN: f32 = 100.0;
/// Largest `pan` component, in texels per second.
pub const MAX_VISTA_PAN: f32 = 1.0e4;

/// Largest fog distance a level may ask for.
pub const MAX_FOG_DEPTH: f32 = 1000.0;

/// Unit vector a projector at `yaw` and `pitch` shines along.
pub fn projector_dir(yaw: f32, pitch: f32) -> Vec3 {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    Vec3::new(cp * cy, cp * sy, sp)
}

fn coord_ok(p: Vec3) -> bool {
    p.is_finite()
        && p.x.abs() <= MAX_ENTITY_COORD
        && p.y.abs() <= MAX_ENTITY_COORD
        && p.z.abs() <= MAX_ENTITY_COORD
}

fn radius_ok(r: f32) -> bool {
    r.is_finite() && r > 0.0 && r <= MAX_ENTITY_COORD
}

fn unit_ok(v: f32) -> bool {
    (0.0..=1.0).contains(&v)
}

fn within(v: f32, limit: f32) -> bool {
    v.is_finite() && v.abs() <= limit
}

/// The checks of [`Entity::VistaLayer`]'s numbers, apart from its two points.
fn vista_ok(slot: u8, layer: u8, parallax: f32, spin: Vec3, pan: [f32; 2], fog: [f32; 2]) -> bool {
    slot < VIEW_SLOTS
        && layer < MAX_VISTA_LAYERS
        && unit_ok(parallax)
        && [spin.x, spin.y, spin.z]
            .into_iter()
            .all(|s| within(s, MAX_VISTA_SPIN))
        && pan.into_iter().all(|p| within(p, MAX_VISTA_PAN))
        && fog[0].is_finite()
        && fog[0] >= 0.0
        && fog[1] > fog[0]
        && fog[1] <= MAX_VISTA_FOG
}

impl Entity {
    /// Whether every field is finite and in range.
    pub fn is_valid(&self) -> bool {
        match *self {
            Entity::PlayerStart { pos, yaw } => coord_ok(pos) && yaw.is_finite(),
            Entity::Light {
                pos,
                color,
                radius,
                intensity,
            } => {
                coord_ok(pos)
                    && unit_ok(color.x)
                    && unit_ok(color.y)
                    && unit_ok(color.z)
                    && radius_ok(radius)
                    && (0.0..=MAX_ENTITY_COORD).contains(&intensity)
            }
            Entity::Projector {
                pos,
                yaw,
                pitch,
                color,
                radius,
                intensity,
                inner,
                outer,
            } => {
                let half_turn = core::f32::consts::FRAC_PI_2;
                coord_ok(pos)
                    && yaw.is_finite()
                    && (-half_turn..=half_turn).contains(&pitch)
                    && [color.x, color.y, color.z].into_iter().all(unit_ok)
                    && radius_ok(radius)
                    && (0.0..=MAX_ENTITY_COORD).contains(&intensity)
                    && inner >= 0.0
                    && inner <= outer
                    && outer > 0.0
                    && outer <= half_turn
            }
            Entity::Enemy { pos, .. } => coord_ok(pos),
            Entity::Goal { pos, radius } => coord_ok(pos) && radius_ok(radius),
            Entity::World {
                pos,
                fog_start,
                fog_end,
            } => {
                coord_ok(pos)
                    && fog_start.is_finite()
                    && fog_start >= 0.0
                    && fog_end > fog_start
                    && fog_end <= MAX_FOG_DEPTH
            }
            Entity::Marker { pos, yaw } => coord_ok(pos) && yaw.is_finite(),
            Entity::Zone { pos, half } => {
                coord_ok(pos) && radius_ok(half.x) && radius_ok(half.y) && radius_ok(half.z)
            }
            Entity::VistaLayer {
                pos,
                centre,
                slot,
                layer,
                parallax,
                spin,
                pan,
                fog_start,
                fog_end,
                ..
            } => {
                coord_ok(pos)
                    && coord_ok(centre)
                    && vista_ok(slot, layer, parallax, spin, pan, [fog_start, fog_end])
            }
        }
    }

    /// The entity's position.
    pub fn pos(&self) -> Vec3 {
        match *self {
            Entity::PlayerStart { pos, .. }
            | Entity::Light { pos, .. }
            | Entity::Projector { pos, .. }
            | Entity::Enemy { pos, .. }
            | Entity::Goal { pos, .. }
            | Entity::World { pos, .. }
            | Entity::Marker { pos, .. }
            | Entity::Zone { pos, .. }
            | Entity::VistaLayer { pos, .. } => pos,
        }
    }
}

impl Level {
    /// The fog range `(start, end)` of the level's first [`Entity::World`].
    pub fn fog(&self) -> Option<(f32, f32)> {
        self.entities.iter().find_map(|e| match *e {
            Entity::World {
                fog_start, fog_end, ..
            } => Some((fog_start, fog_end)),
            _ => None,
        })
    }
}

impl Level {
    /// Where the first player start is, which is the one that counts.
    pub fn player_start(&self) -> Option<Vec3> {
        self.entities.iter().find_map(|e| match *e {
            Entity::PlayerStart { pos, .. } => Some(pos),
            _ => None,
        })
    }

    /// The vista layers, the first of each `(slot, layer)` only, in file order.
    /// A later entity for the same place is ignored, as with [`Entity::World`].
    pub fn vista_layers(&self) -> Vec<&Entity> {
        let mut seen = [[false; MAX_VISTA_LAYERS as usize]; VIEW_SLOTS as usize];
        let mut out = Vec::new();
        for e in &self.entities {
            let Entity::VistaLayer { slot, layer, .. } = *e else {
                continue;
            };
            // `is_valid` bounds both, but a level built in code may not have been
            // checked, so an out-of-range entity is skipped rather than indexed.
            let Some(taken) = seen
                .get_mut(usize::from(slot))
                .and_then(|row| row.get_mut(usize::from(layer)))
            else {
                continue;
            };
            if !std::mem::replace(taken, true) {
                out.push(e);
            }
        }
        out
    }
}

/// A level's source data: brushes plus entities. Compiled data (BSP,
/// portals) is derived from the brushes on load rather than stored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Level {
    pub brushes: Vec<Brush>,
    pub entities: Vec<Entity>,
    /// Voxel volumes, none of which overlap. Their CSG order shares `Brush::order`.
    pub volumes: Vec<Volume>,
    /// Names and tags of entities, for scripts.
    pub labels: Vec<EntityLabel>,
    /// Named brushes that move; they stay out of the CSG.
    pub mover_groups: Vec<MoverGroup>,
}

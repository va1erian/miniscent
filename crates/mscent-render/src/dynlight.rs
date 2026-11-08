//! Short-lived point lights (a muzzle flash, an explosion) that light walls
//! and meshes on top of the baked light.
//!
//! A light is placed by [`DynLight::place`], which reads the row of the level's
//! room table for the room it is in and keeps which rooms it reaches
//! ([`LightReach`]), so a frame only tests a bit per face. Placing it again
//! somewhere else, for a light that moves, floods nothing. The renderer takes a
//! slice of lights and never changes them, so frames in parallel bands agree.

use mscent_math::{DPlane, Vec3};

use crate::mesh::PointLight as MeshLight;
use crate::scene::{LightReach, Scene};
use mscent_world::LeafId;

/// Most lights a frame uses; the rest of a longer slice is ignored.
pub const MAX_DYN_LIGHTS: usize = 4;
/// Largest radius a light is given, in world units: the room table holds
/// distances no further.
pub const MAX_RADIUS: f32 = mscent_world::rooms::MAX_LIGHT_REACH;
/// Largest colour component. 1.0 adds as much light as a plain texture
/// colour at point-blank range.
pub const MAX_COLOR: f32 = 16.0;

/// A point light with linear-squared falloff to zero at `radius`.
#[derive(Clone, Debug, Default)]
pub struct DynLight {
    pos: Vec3,
    radius: f32,
    color: Vec3,
    reach: LightReach,
}

impl DynLight {
    /// A light that lights nothing, until it is placed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts the light at `pos` in `scene` and finds the rooms it reaches. False,
    /// with the light left dark, if a value is not finite, `radius` is not
    /// positive, or `color` has a negative part. A radius over [`MAX_RADIUS`]
    /// and colour parts over [`MAX_COLOR`] are cut down to those.
    pub fn place(&mut self, scene: &Scene, pos: Vec3, radius: f32, color: Vec3) -> bool {
        self.radius = 0.0;
        self.color = Vec3::ZERO;
        self.reach.clear();
        let valid = pos.is_finite() && radius.is_finite() && radius > 0.0 && color_ok(color);
        if !valid {
            return false;
        }
        let radius = radius.min(MAX_RADIUS);
        scene.light_reach(pos, radius, &mut self.reach);
        self.pos = pos;
        self.radius = radius;
        self.set_color(color)
    }

    /// Changes the colour, for a fade. Same rules as in [`Self::place`]; on
    /// false the colour is unchanged.
    pub fn set_color(&mut self, color: Vec3) -> bool {
        let ok = color_ok(color);
        if ok {
            let cap = |c: f32| c.min(MAX_COLOR);
            self.color = Vec3::new(cap(color.x), cap(color.y), cap(color.z));
        }
        ok
    }

    pub fn pos(&self) -> Vec3 {
        self.pos
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn color(&self) -> Vec3 {
        self.color
    }

    /// Whether the light adds anything at all.
    pub fn is_lit(&self) -> bool {
        self.radius > 0.0 && self.color != Vec3::ZERO
    }

    /// Whether a face is in front of a room the light reaches. The shading
    /// gives a face that is too far, or turned away, nothing.
    pub fn reaches_face(&self, face: usize) -> bool {
        self.reach.has_face(face)
    }

    /// Whether `plane` has the light on its front side and within the radius:
    /// what a face needs besides a room the light reaches to take any of it.
    pub fn lights_plane(&self, plane: &DPlane) -> bool {
        let d = plane.signed_distance(self.pos.to_f64());
        d > 0.0 && d < f64::from(self.radius)
    }

    /// Whether the light reached the room of `leaf`.
    pub fn reaches_leaf(&self, leaf: LeafId) -> bool {
        self.reach.has_leaf(leaf)
    }

    /// Whether the light reached a room of face set `set`
    /// ([`RoomMap::set_rooms`](crate::scene::RoomMap::set_rooms)), as
    /// `reaches_face` asks for every face with that set at once.
    pub fn reaches_set(&self, set: u32) -> bool {
        self.reach.has_set(set)
    }

    /// The light as a mesh sees it: one brightness, the mean of the colour.
    pub(crate) fn mesh_light(&self) -> MeshLight {
        MeshLight {
            pos: self.pos,
            radius: self.radius,
            intensity: (self.color.x + self.color.y + self.color.z) / 3.0,
        }
    }
}

fn color_ok(c: Vec3) -> bool {
    c.is_finite() && c.x >= 0.0 && c.y >= 0.0 && c.z >= 0.0
}

#[cfg(test)]
#[path = "dynlight_tests.rs"]
mod dynlight_tests;

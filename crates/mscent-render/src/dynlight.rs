//! Short-lived point lights (a muzzle flash, an explosion) that light walls
//! and meshes on top of the baked light.
//!
//! A light is placed once: [`DynLight::place`] floods the portals from its
//! leaf and keeps which leaves and faces it reaches ([`LightReach`]), so a
//! frame only tests a bit per face. The renderer takes a slice of lights and
//! never changes them, so frames in parallel bands agree.

use mscent_math::Vec3;

use crate::mesh::PointLight as MeshLight;
use crate::scene::{LightReach, Scene};
use mscent_world::LeafId;

/// Most lights a frame uses; the rest of a longer slice is ignored.
pub const MAX_DYN_LIGHTS: usize = 4;
/// Largest radius a light is given, in world units: it bounds the flood.
pub const MAX_RADIUS: f32 = 64.0;
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

    /// Puts the light at `pos` in `scene` and finds what it reaches. False,
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

    /// Whether a face is within reach of the light and faces it.
    pub fn reaches_face(&self, face: usize) -> bool {
        self.reach.has_face(face)
    }

    /// Whether the light reached `leaf` through the portals.
    pub fn reaches_leaf(&self, leaf: LeafId) -> bool {
        self.reach.has_leaf(leaf)
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

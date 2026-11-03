//! Enemy models beside the level (design in `docs/enemies.md`). The triangle fill
//! and the shading are the voxel path's, in a model's own space: its eye, its light
//! direction and its texture axes all turn with it, so a texture rides on the body.
//! Only the corners' screen positions and the light at each vertex are worked out
//! in the world.
//!
//! * [`ModelTable`]: built models, as drawing wants them. Made at load.
//! * [`ModelList`] (`list.rs`): a frame's parts, with their level of detail and
//!   their vertices projected and lit once, before any band starts.
//! * [`fill_models`] (`fill.rs`): filling the rows a band holds.

mod fill;
mod list;

#[cfg(test)]
mod tests;

use mscent_math::{Quat, Vec3};
use mscent_world::model::{Blueprint, LEVELS, Model, PartMesh, PartVertex};
use mscent_world::voxel::FALLBACK_MATERIAL;

use crate::material::MaterialTable;
use crate::voxel::vertex_axis;

pub(crate) use fill::{ModelFill, fill_models};
pub use list::MAX_MODEL_TRIS;
pub(crate) use list::ModelList;

/// Which model of a [`ModelTable`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelId(pub u32);

/// A model placed in a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelInstance {
    pub model: ModelId,
    pub pos: Vec3,
    pub rot: Quat,
    /// On top of the scale the blueprint gives.
    pub scale: f32,
    /// Multiplies its light, per channel: 1 leaves it as it is. Costs nothing per
    /// pixel, so one design can be elite, wounded or of another faction.
    pub tint: Vec3,
}

impl ModelInstance {
    /// A model as designed, at `pos`, turned by `rot`.
    pub fn new(model: ModelId, pos: Vec3, rot: Quat) -> Self {
        Self {
            model,
            pos,
            rot,
            scale: 1.0,
            tint: Vec3::new(1.0, 1.0, 1.0),
        }
    }
}

/// One level of detail of a part, with what drawing needs for each vertex.
#[derive(Clone, Debug, Default)]
pub(crate) struct Level {
    pub vertices: Vec<PartVertex>,
    pub triangles: Vec<[u16; 3]>,
    /// Whether each triangle glows.
    pub glow: Vec<bool>,
    /// Which model axis textures each vertex, or that it lies between two.
    pub axis: Vec<u8>,
    /// The material each vertex is textured with: that of its first triangle.
    pub material: Vec<u8>,
}

/// One built model, as drawing wants it.
#[derive(Clone, Debug)]
pub(crate) struct Prepared {
    /// Levels of each part.
    pub parts: Vec<[Level; LEVELS]>,
    pub scale: f32,
    /// Farthest any vertex is from the origin, in model units.
    pub radius: f32,
}

/// The models a game may draw. Made once: nothing here changes per frame.
#[derive(Clone, Debug, Default)]
pub struct ModelTable {
    models: Vec<Prepared>,
}

impl ModelTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a built model. A material `materials` does not have, or that a voxel
    /// surface cannot name, draws grey like any other unknown material.
    pub fn add(
        &mut self,
        blueprint: &Blueprint,
        model: &Model,
        materials: &MaterialTable,
    ) -> ModelId {
        let id = ModelId(self.models.len() as u32);
        let roles: Vec<(u8, bool)> = blueprint
            .skins
            .iter()
            .map(|s| {
                let found = materials.find(&s.material);
                let usable = found.and_then(|m| u8::try_from(m.0).ok());
                let id = usable.filter(|&m| m < FALLBACK_MATERIAL);
                (id.unwrap_or(FALLBACK_MATERIAL), s.glow)
            })
            .collect();
        let parts: Vec<[Level; LEVELS]> = model
            .parts
            .iter()
            .map(|p| [0, 1, 2].map(|l| level_of(&p.lods[l], &roles)))
            .collect();
        let reach = |l: &Level| {
            l.vertices
                .iter()
                .map(|v| v.pos.length())
                .fold(0.0, f32::max)
        };
        let radius = parts.iter().map(|p| reach(&p[0])).fold(0.0, f32::max);
        self.models.push(Prepared {
            parts,
            scale: blueprint.scale as f32,
            radius,
        });
        id
    }

    pub fn len(&self) -> usize {
        self.models.len()
    }

    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    pub(crate) fn get(&self, id: u32) -> Option<&Prepared> {
        self.models.get(id as usize)
    }
}

fn level_of(mesh: &PartMesh, roles: &[(u8, bool)]) -> Level {
    let role = |r: u8| {
        roles
            .get(usize::from(r))
            .copied()
            .unwrap_or((FALLBACK_MATERIAL, false))
    };
    let mut material = vec![None; mesh.vertices.len()];
    for (t, &r) in mesh.triangles.iter().zip(&mesh.roles) {
        for &i in t {
            if let Some(slot) = material.get_mut(usize::from(i)) {
                slot.get_or_insert(role(r).0);
            }
        }
    }
    Level {
        vertices: mesh.vertices.clone(),
        triangles: mesh.triangles.clone(),
        glow: mesh.roles.iter().map(|&r| role(r).1).collect(),
        axis: mesh
            .vertices
            .iter()
            .map(|v| vertex_axis(v.normal))
            .collect(),
        material: material
            .into_iter()
            .map(|m| m.unwrap_or(FALLBACK_MATERIAL))
            .collect(),
    }
}

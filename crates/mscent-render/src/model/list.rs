//! The part of a model frame that does not depend on the rows being drawn: which
//! parts, at which level of detail, with their vertices projected and lit once so
//! that bands share them read-only. Nothing here allocates once its lists have grown.

use mscent_math::{Aabb, Mat3, Vec3};
use mscent_world::LeafId;

use super::{ModelInstance, ModelTable, Prepared};
use crate::camera::{Camera, Frustum};
use crate::dynlight::DynLight;
use crate::mesh::MeshLighting;
use crate::scene::eye_of;
use crate::voxel::list::{ScreenV, in_view, project, row_span};

/// Triangles a frame submits at the level its distance asks for. Past this, the
/// farthest models take their lowest level, so an overloaded view loses distant
/// detail first. It is not a hard limit: nothing is left out of the picture.
pub const MAX_MODEL_TRIS: u32 = 8000;
/// Radius on screen, in pixels, below which a model takes half, then quarter detail.
const LEVEL_PIXELS: [f32; 2] = [48.0, 20.0];
/// How much larger than its switching size a model must get to come back, so one
/// near the line does not flip every frame.
const LEVEL_HYSTERESIS: f32 = 1.15;
/// The lowest level of detail.
const LOWEST: u8 = 2;
/// Light units of the plain texture colour.
const PLAIN: f32 = 256.0;

/// Where a model is: from its own units to the world.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placement {
    rot: Mat3,
    pos: Vec3,
    scale: f32,
}

impl Placement {
    pub(crate) fn point(&self, p: Vec3) -> Vec3 {
        self.rot.mul_vec(p * self.scale) + self.pos
    }

    fn direction(&self, d: Vec3) -> Vec3 {
        self.rot.mul_vec(d)
    }

    fn inverse_point(&self, p: Vec3) -> Vec3 {
        self.rot.transpose().mul_vec(p - self.pos) * (1.0 / self.scale)
    }

    fn inverse_direction(&self, d: Vec3) -> Vec3 {
        self.rot.transpose().mul_vec(d)
    }
}

/// One part of a frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ListedPart {
    pub model: u32,
    pub part: u32,
    pub lod: u8,
    /// Where its vertices start in the frame's lists.
    pub start: usize,
    /// Screen rows `first..last` it may cover.
    pub first: i32,
    pub last: i32,
    pub place: Placement,
    /// The eye and the direction of the light, in model units.
    pub eye: Vec3,
    pub dir: Vec3,
}

/// A frame's model parts, and what is kept from one frame to the next.
#[derive(Default)]
pub(crate) struct ModelList {
    pub parts: Vec<ListedPart>,
    pub screen: Vec<ScreenV>,
    pub light: Vec<Vec3>,
    pub triangles: u32,
    /// Those in view, with how far off they are and their instance.
    near: Vec<(f32, u32)>,
    /// By instance: its level of detail last frame.
    lods: Vec<u8>,
    /// The level's lighting plus this frame's dynamic lights on one instance.
    lighting: MeshLighting,
}

impl ModelList {
    /// Empties the list, keeping its storage for a frame with models.
    pub(crate) fn clear(&mut self) {
        self.parts.clear();
        self.screen.clear();
        self.light.clear();
        self.near.clear();
        self.triangles = 0;
    }

    /// Replaces the list. `lit` lights every model, `lights` the ones in a leaf they
    /// reach, and `leaf_at` says which leaf a point is in.
    pub(crate) fn prepare(
        &mut self,
        table: &ModelTable,
        instances: &[ModelInstance],
        cam: &Camera,
        lit: &MeshLighting,
        lights: &[DynLight],
        leaf_at: impl Fn(Vec3) -> LeafId,
    ) {
        self.clear();
        self.lods.resize(instances.len(), 0);
        self.lighting.clone_from(lit);
        let base = lit.points.len();
        let (eye, frustum) = (eye_of(cam), cam.frustum());
        for (i, instance) in (0u32..).zip(instances) {
            if let Some(d) = distance_in_view(table, instance, cam, &frustum, eye) {
                self.near.push((d, i));
            }
        }
        self.near.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let near = std::mem::take(&mut self.near);
        for &(d, i) in &near {
            let instance = &instances[i as usize];
            let Some(model) = table.get(instance.model.0) else {
                continue;
            };
            self.lighting.points.truncate(base);
            if lights.iter().any(DynLight::is_lit) {
                let leaf = leaf_at(instance.pos);
                let reaching = lights
                    .iter()
                    .filter(|l| l.is_lit() && l.reaches_leaf(leaf))
                    .map(DynLight::mesh_light);
                self.lighting.points.extend(reaching);
            }
            let lod = self.pick_level(i as usize, model, instance, d, cam.focal());
            self.take(instance, model, lod, cam, eye);
        }
        self.near = near;
    }

    /// Level of detail by size on screen. Once a frame has submitted its budget,
    /// the rest (the farthest, as models come nearest first) take the lowest.
    fn pick_level(
        &mut self,
        i: usize,
        model: &Prepared,
        instance: &ModelInstance,
        distance: f32,
        focal: f32,
    ) -> u8 {
        let radius = model.radius * model.scale * instance.scale;
        let pixels = radius * focal / distance.max(1e-3);
        let last = self.lods[i];
        let finer_than = |level: u8| {
            let line = LEVEL_PIXELS[usize::from(level)];
            if last > level {
                pixels > line * LEVEL_HYSTERESIS
            } else {
                pixels > line
            }
        };
        let wanted = if finer_than(0) {
            0
        } else if finer_than(1) {
            1
        } else {
            LOWEST
        };
        let lod = if self.triangles >= MAX_MODEL_TRIS {
            LOWEST
        } else {
            wanted
        };
        self.lods[i] = lod;
        lod
    }

    fn take(
        &mut self,
        instance: &ModelInstance,
        model: &Prepared,
        lod: u8,
        cam: &Camera,
        eye: Vec3,
    ) {
        let place = Placement {
            rot: instance.rot.normalized().to_mat3(),
            pos: instance.pos,
            scale: model.scale * instance.scale,
        };
        let world_dir = dominant_direction(&self.lighting, instance.pos);
        let (model_eye, dir) = (place.inverse_point(eye), place.inverse_direction(world_dir));
        for (p, levels) in (0u32..).zip(&model.parts) {
            let level = &levels[usize::from(lod)];
            let start = self.screen.len();
            let (lighting, tint) = (&self.lighting, instance.tint);
            self.screen.extend(
                level
                    .vertices
                    .iter()
                    .map(|v| project(cam, place.point(v.pos))),
            );
            self.light.extend(level.vertices.iter().map(|v| {
                let k = lighting.at(place.point(v.pos), place.direction(v.normal)) * PLAIN;
                Vec3::new(k * tint.x, k * tint.y, k * tint.z)
            }));
            let (first, last) = row_span(&self.screen[start..], cam.height() as i32);
            self.triangles = self.triangles.saturating_add(level.triangles.len() as u32);
            self.parts.push(ListedPart {
                model: instance.model.0,
                part: p,
                lod,
                start,
                first,
                last,
                place,
                eye: model_eye,
                dir,
            });
        }
    }
}

/// How far an instance is, if its pose can be drawn and any of it can be seen.
fn distance_in_view(
    table: &ModelTable,
    instance: &ModelInstance,
    cam: &Camera,
    frustum: &Frustum,
    eye: Vec3,
) -> Option<f32> {
    let model = table.get(instance.model.0)?;
    let q = instance.rot;
    let finite = [q.x, q.y, q.z, q.w, instance.scale]
        .iter()
        .all(|v| v.is_finite());
    if !finite || !instance.pos.is_finite() || !instance.tint.is_finite() {
        return None;
    }
    if instance.scale <= 0.0 || q.norm_squared() <= 0.0 {
        return None;
    }
    let radius = model.radius * model.scale * instance.scale;
    let r = Vec3::new(radius, radius, radius);
    let bounds = Aabb::new(instance.pos - r, instance.pos + r);
    in_view(cam, frustum, &bounds).then(|| (instance.pos - eye).length())
}

/// Where most of the light comes from, by its strength at `pos`, as a unit vector.
/// A model has one, so its bump and highlight need no lamp loop per pixel.
fn dominant_direction(lighting: &MeshLighting, pos: Vec3) -> Vec3 {
    let mut sum = lighting.sun_dir * lighting.sun;
    for l in &lighting.points {
        let to = l.pos - pos;
        let d = to.length();
        if d < l.radius && d > 1e-4 {
            let fall = 1.0 - d / l.radius;
            sum = sum + to * (l.intensity * fall * fall / d);
        }
    }
    sum.normalized().unwrap_or(Vec3::new(0.0, 0.0, 1.0))
}
